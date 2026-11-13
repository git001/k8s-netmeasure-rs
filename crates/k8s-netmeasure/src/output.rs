//! Atomic file write (design §5): write a temporary file in the target directory,
//! fsync, rename, fsync the directory. Readers see either no file or a complete one,
//! and after the final directory fsync the new file also survives a node crash.

use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic_as(path, bytes, &std::process::id().to_string())
}

/// Like [`write_atomic`]; the temporary file is `<name>.tmp-<tag>` next to the target
/// (`run` passes its `run_id`, design §5).
pub fn write_atomic_as(path: &Path, bytes: &[u8], tag: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("output path has no file name"))?;
    let tmp = dir.join(format!("{}.tmp-{tag}", name.to_string_lossy()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    // Persists the directory entry of the rename.
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let dir = std::env::temp_dir().join(format!("nm-out-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "no temp file left"
        );
        assert!(write_atomic(&dir.join("missing/x.json"), b"x").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
