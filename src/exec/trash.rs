use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Component, Path};

pub fn to_trash(home: &Path, source: &Path, run_id: &str, sequence: usize) -> io::Result<()> {
    let trash = home.join(".local/share/Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    ensure_directory(home, &files)?;
    ensure_directory(home, &info)?;
    set_private_directory(&files)?;
    set_private_directory(&info)?;
    let source_metadata = fs::symlink_metadata(source)?;
    if source_metadata.dev() != fs::metadata(&files)?.dev() && source_metadata.file_type().is_file()
    {
        check_cross_device_space(&files, source_metadata.len())?;
    }

    let file_name = source
        .file_name()
        .ok_or_else(|| io::Error::other("trash source has no file name"))?;
    let encoded_name = percent_encode(file_name.as_bytes());
    for attempt in 0..100 {
        let name = format!("{run_id}-{sequence}-{attempt}-{encoded_name}");
        let destination = files.join(&name);
        let info_path = info.join(format!("{name}.trashinfo"));
        let mut info_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let record = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            percent_encode(source.as_os_str().as_bytes()),
            trash_date(run_id)
        );
        if let Err(error) = info_file
            .write_all(record.as_bytes())
            .and_then(|()| info_file.sync_all())
        {
            let _ = fs::remove_file(&info_path);
            return Err(error);
        }
        if let Err(error) = set_private_file(&info_path) {
            let _ = fs::remove_file(&info_path);
            return Err(error);
        }

        match move_noreplace(source, &destination) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&info_path);
            }
            Err(error) => {
                let _ = fs::remove_file(&info_path);
                return Err(error);
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique trash entry",
    ))
}

fn check_cross_device_space(destination: &Path, copy_size: u64) -> io::Result<()> {
    let destination_c = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("trash path contains a NUL byte"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: destination_c is NUL-terminated and stats points to writable storage.
    if unsafe { libc::statvfs(destination_c.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statvfs initialized stats after returning success.
    let stats = unsafe { stats.assume_init() };
    let unit = u128::from(stats.f_frsize);
    let available = u128::from(stats.f_bavail) * unit;
    let total = u128::from(stats.f_blocks) * unit;
    if !enough_space(available, total, copy_size) {
        return Err(io::Error::other(
            "insufficient free space to safely stage cross-filesystem trash item",
        ));
    }
    Ok(())
}

fn enough_space(available: u128, total: u128, copy_size: u64) -> bool {
    let reserve = total / 20 + 512 * 1024 * 1024;
    available >= u128::from(copy_size) + reserve
}

fn ensure_directory(home: &Path, directory: &Path) -> io::Result<()> {
    if !directory.starts_with(home) {
        return Err(io::Error::other("trash directory is outside the home"));
    }
    let relative = directory
        .strip_prefix(home)
        .map_err(|_| io::Error::other("trash directory is outside the home"))?;
    let mut current = home.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(name) = component {
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(_) => return Err(io::Error::other("trash path contains a non-directory")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&current)?;
                    set_private_directory(&current)?;
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

fn move_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source_c = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("source path contains a NUL byte"))?;
    let destination_c = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::other("trash path contains a NUL byte"))?;
    // SAFETY: both C strings are NUL-terminated and remain alive for the call;
    // renameat2 accesses only the named paths and RENAME_NOREPLACE prevents
    // overwriting an existing trash entry.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source_c.as_ptr(),
            libc::AT_FDCWD,
            destination_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EXDEV) {
        copy_across_filesystems(source, destination)
    } else {
        Err(error)
    }
}

fn copy_across_filesystems(source: &Path, destination: &Path) -> io::Result<()> {
    let before = fs::symlink_metadata(source)?;
    if before.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        std::os::unix::fs::symlink(target, destination)?;
        let after = fs::symlink_metadata(source)?;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            let _ = fs::remove_file(destination);
            return Err(io::Error::other("trash source changed during move"));
        }
        return fs::remove_file(source).inspect_err(|_| {
            let _ = fs::remove_file(destination);
        });
    }
    if !before.file_type().is_file() {
        return Err(io::Error::other(
            "cross-filesystem trash supports files and symlinks only",
        ));
    }
    let source_modified = before.modified()?;
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)?;
    let opened = input.metadata()?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(io::Error::other("trash source changed during move"));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    if let Err(error) = fs::set_permissions(
        destination,
        fs::Permissions::from_mode(before.mode() & 0o777),
    ) {
        drop(output);
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    drop(output);
    let after = match fs::symlink_metadata(source) {
        Ok(after) => after,
        Err(error) => {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
    };
    let after_modified = match after.modified() {
        Ok(modified) => modified,
        Err(error) => {
            let _ = fs::remove_file(destination);
            return Err(error);
        }
    };
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || source_modified != after_modified
    {
        let _ = fs::remove_file(destination);
        return Err(io::Error::other("trash source changed during move"));
    }
    fs::remove_file(source).inspect_err(|_| {
        let _ = fs::remove_file(destination);
    })
}

fn percent_encode(bytes: &[u8]) -> String {
    let mut result = String::new();
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

fn trash_date(run_id: &str) -> String {
    if run_id.len() >= 15 {
        format!(
            "{}-{}-{}T{}:{}:{}",
            &run_id[0..4],
            &run_id[4..6],
            &run_id[6..8],
            &run_id[9..11],
            &run_id[11..13],
            &run_id[13..15]
        )
    } else {
        run_id.to_string()
    }
}

fn set_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn set_private_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_device_space_check_preserves_five_percent_and_512_mib() {
        let total = 20 * 1024 * 1024 * 1024_u128;
        let reserve = total / 20 + 512 * 1024 * 1024;
        let copy_size = 100 * 1024 * 1024_u64;
        assert!(enough_space(
            reserve + u128::from(copy_size),
            total,
            copy_size
        ));
        assert!(!enough_space(
            reserve + u128::from(copy_size) - 1,
            total,
            copy_size
        ));
    }
}
