// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::io;
use std::path::Path;

use same_file::Handle;

/// Checks ordinary-file status and identity using the opened handles
/// themselves.
///
/// # Parameters
pub(super) fn validate_files(files: &[(&'static str, &File)]) -> io::Result<()> {
    let mut identities = Vec::with_capacity(files.len());
    for (name, file) in files {
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} must refer to a regular file"),
            ));
        }
        let identity = Handle::from_file(file.try_clone()?)?;
        if identities.iter().any(|(_, previous)| previous == &identity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{name} conflicts with another configured file"),
            ));
        }
        identities.push((*name, identity));
    }
    Ok(())
}

/// Rejects collisions between owned handles and configured legacy tee paths.
///
/// # Parameters
/// * `files`: Supplies the files value used by this operation.
/// * `tee_paths`: Supplies the tee paths value used by this operation.
pub(super) fn validate_tee_paths(
    files: &[(&'static str, &File)],
    tee_paths: &[(&'static str, &Path)],
) -> io::Result<()> {
    let owned = files
        .iter()
        .map(|(name, file)| {
            let identity = Handle::from_file(file.try_clone()?)?;
            Ok((*name, identity))
        })
        .collect::<io::Result<Vec<_>>>()?;
    for (tee_name, path) in tee_paths {
        match std::fs::metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{tee_name} tee must refer to a regular file"),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
            _ => {}
        }
        let tee_identity = Handle::from_file(open_tee_identity(path)?)?;
        if owned.iter().any(|(_, identity)| identity == &tee_identity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{tee_name} tee conflicts with an owned file"),
            ));
        }
    }
    Ok(())
}

/// Stores the open tee identity value used by this execution component.
///
/// # Parameters
/// * `path`: Supplies the path value used by this operation.
fn open_tee_identity(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}

/// Resolves a relative path against a captured working directory lexically.
///
/// # Parameters
/// * `base`: Captured working directory used as the resolution root.
/// * `path`: Path to resolve without consulting the filesystem.
pub(crate) fn resolve_io_path(base: &Path, path: &Path) -> std::path::PathBuf {
    if path.is_absolute() || has_windows_drive_prefix(path) {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(windows)]
fn has_windows_drive_prefix(path: &Path) -> bool {
    use std::path::Component;
    matches!(path.components().next(), Some(Component::Prefix(_)))
}

#[cfg(not(windows))]
fn has_windows_drive_prefix(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::path::Path;
    #[cfg(unix)]
    use std::fs::File;

    #[cfg(windows)]
    use super::resolve_io_path;
    use super::validate_files;

    #[cfg(windows)]
    #[test]
    fn test_drive_relative_path_keeps_windows_drive_semantics() {
        let path = Path::new(r"C:folder\input.txt");
        assert_eq!(resolve_io_path(Path::new(r"D:\parent"), path).as_path(), path);
    }

    #[test]
    fn test_identity_uses_open_handles_and_detects_cloned_file_aliases() {
        let file = tempfile::tempfile().expect("temporary file should be created");
        let alias = file.try_clone().expect("file handle should be cloned");
        let error = validate_files(&[("stdin", &file), ("stdout", &alias)])
            .expect_err("the same file identity must be rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_identity_rejects_non_regular_handles() {
        #[cfg(unix)]
        let directory = File::open(std::env::temp_dir()).expect("temp directory should open");
        #[cfg(windows)]
        let directory = {
            use std::os::windows::fs::OpenOptionsExt;

            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(std::env::temp_dir())
                .expect("temp directory should open")
        };
        let error = validate_files(&[("stdin", &directory)]).expect_err("directories are not valid file streams");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
