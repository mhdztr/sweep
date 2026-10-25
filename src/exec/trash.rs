//! XDG trash (stub for M2 wiring).

use std::path::Path;

pub fn to_trash(_src: &Path) -> std::io::Result<std::path::PathBuf> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "trash not implemented",
    ))
}
