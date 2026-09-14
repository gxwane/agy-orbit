use crate::error::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Write data to a destination path atomically using a temporary file in the same directory,
/// calling sync_all() prior to persist to ensure crash-resilient durability.
pub fn atomic_write<P: AsRef<Path>, C: AsRef<[u8]>>(dest: P, contents: C) -> Result<()> {
    let dest = dest.as_ref();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let parent_dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".agyo_tmp_")
        .tempfile_in(parent_dir)?;

    temp.write_all(contents.as_ref())?;
    temp.flush()?;

    // Sync disk buffers to guarantee crash durability
    temp.as_file().sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o600));
    }

    let persist_res = temp.persist(dest);
    if let Err(mut persist_err) = persist_res {
        let is_retryable = persist_err
            .error
            .raw_os_error()
            .is_some_and(|code| code == 32 || code == 5);

        if is_retryable {
            for attempt in 1..=3 {
                std::thread::sleep(std::time::Duration::from_millis(10 * (1 << attempt)));
                match persist_err.file.persist(dest) {
                    Ok(_) => return Ok(()),
                    Err(next_err) => {
                        persist_err = next_err;
                    }
                }
            }
        }

        return Err(persist_err.error.into());
    }
    Ok(())
}

/// Atomically copy a file from src to dest.
pub fn atomic_copy<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dest: Q) -> Result<()> {
    let content = fs::read(src)?;
    atomic_write(dest, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_and_copy() {
        let dir = tempdir().unwrap();
        let src_file = dir.path().join("src.txt");
        let dest_file = dir.path().join("dest.txt");

        atomic_write(&src_file, b"test content").unwrap();
        assert_eq!(fs::read_to_string(&src_file).unwrap(), "test content");

        atomic_copy(&src_file, &dest_file).unwrap();
        assert_eq!(fs::read_to_string(&dest_file).unwrap(), "test content");
    }
}
