//! Writing a file by replacing it, so a reader never sees half of one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `contents` to a temporary sibling of `path`, then renames it over `path`.
///
/// The way 3D Review writes its `--report`: a rename within one folder is the closest thing
/// either OS has to an atomic swap, and `fs::rename` replaces an existing file on Windows as well
/// as Unix. On any failure the temporary file is removed and `path` is left as it was.
pub(crate) fn write_replacing(path: &Path, contents: &[u8]) -> io::Result<()> {
    let staging = staging_path(path);
    // One left behind by a killed process.
    let _ = fs::remove_file(&staging);
    let result = fs::write(&staging, contents).and_then(|()| fs::rename(&staging, path));
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

/// `<name>.partial-<pid>-<n>` beside `path`: unique per process and per write, so two writers
/// aimed at one path never share a temporary file.
fn staging_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".partial-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    path.with_file_name(name)
}
