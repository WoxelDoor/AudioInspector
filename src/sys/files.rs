//! Output files. Reports and captures go next to the exe, a folder the signed-in user
//! can write to; when AudioInspector runs as administrator, a link planted there must
//! not redirect the write, and an existing file is never opened or truncated.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Creates `<dir>\<stem><ext>` as a new file, or `<stem>-2<ext>`, `-3`… when the name is
/// taken. Refuses when `dir` is a junction or a symbolic link.
pub fn create_new_in(dir: &Path, stem: &str, ext: &str) -> io::Result<(File, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} is a link, nothing was written", dir.display())));
    }
    for n in 1..=50 {
        let name = if n == 1 { format!("{stem}{ext}") } else { format!("{stem}-{n}{ext}") };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free file name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("audioinspector-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_taken_name_gets_a_number_and_is_not_overwritten() {
        let dir = scratch("names");
        let (mut f, first) = create_new_in(&dir.join("reports"), "report", ".txt").unwrap();
        f.write_all(b"first").unwrap();
        drop(f);
        let (_, second) = create_new_in(&dir.join("reports"), "report", ".txt").unwrap();
        assert!(first.ends_with("report.txt"));
        assert!(second.ends_with("report-2.txt"));
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_junction_in_place_of_the_folder_is_refused() {
        let dir = scratch("junction");
        let target = dir.join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        let link = dir.join("captures");
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&target).output().unwrap();
        assert!(made.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&made.stdout));
        let err = create_new_in(&link, "bt-capture", ".txt").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0, "nothing may land behind the link");
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
