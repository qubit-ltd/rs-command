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

use super::identity_failure::IdentityFailure;

/// Checks ordinary-file status and identity using the opened handles
/// themselves.
///
/// # Parameters
pub(super) fn validate_files(files: &[(crate::FilePreparationDirection, &File)]) -> Result<(), IdentityFailure> {
    let mut identities = Vec::with_capacity(files.len());
    for (direction, file) in files {
        let name = direction_name(*direction);
        if !file
            .metadata()
            .map_err(|source| IdentityFailure {
                direction: *direction,
                source,
            })?
            .file_type()
            .is_file()
        {
            return Err(IdentityFailure {
                direction: *direction,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must refer to a regular file"),
                ),
            });
        }
        let identity = Handle::from_file(file.try_clone().map_err(|source| IdentityFailure {
            direction: *direction,
            source,
        })?)
        .map_err(|source| IdentityFailure {
            direction: *direction,
            source,
        })?;
        if identities.iter().any(|(_, previous)| previous == &identity) {
            return Err(IdentityFailure {
                direction: *direction,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} conflicts with another configured file"),
                ),
            });
        }
        identities.push((*direction, identity));
    }
    Ok(())
}

/// Rejects collisions between owned handles and configured legacy tee paths.
///
/// # Parameters
/// * `files`: Supplies the files value used by this operation.
/// * `tee_paths`: Supplies the tee paths value used by this operation.
pub(super) fn validate_tee_paths(
    files: &[(crate::FilePreparationDirection, &File)],
    tee_paths: &[(crate::FilePreparationDirection, &Path)],
) -> Result<(), IdentityFailure> {
    if tee_paths.is_empty() {
        return Ok(());
    }
    let owned = files
        .iter()
        .map(|(direction, file)| {
            let identity = Handle::from_file(file.try_clone().map_err(|source| IdentityFailure {
                direction: *direction,
                source,
            })?)
            .map_err(|source| IdentityFailure {
                direction: *direction,
                source,
            })?;
            Ok((*direction, identity))
        })
        .collect::<Result<Vec<_>, IdentityFailure>>()?;
    for (direction, path) in tee_paths {
        let tee_name = direction_name(*direction);
        match std::fs::metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(IdentityFailure {
                    direction: *direction,
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{tee_name} tee must refer to a regular file"),
                    ),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(IdentityFailure {
                    direction: *direction,
                    source,
                });
            }
            _ => {}
        }
        let tee_identity = Handle::from_file(open_tee_identity(path).map_err(|source| IdentityFailure {
            direction: *direction,
            source,
        })?)
        .map_err(|source| IdentityFailure {
            direction: *direction,
            source,
        })?;
        if owned.iter().any(|(_, identity)| identity == &tee_identity) {
            return Err(IdentityFailure {
                direction: *direction,
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{tee_name} tee conflicts with an owned file"),
                ),
            });
        }
    }
    Ok(())
}

/// Maps a standard stream name to its diagnostic direction.
///
/// # Parameters
///
/// * `name` - Static stream label used by file identity validation.
fn direction_name(direction: crate::FilePreparationDirection) -> &'static str {
    match direction {
        crate::FilePreparationDirection::Stdin => "stdin",
        crate::FilePreparationDirection::Stdout => "stdout",
        crate::FilePreparationDirection::Stderr => "stderr",
    }
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

/// Returns whether a path begins with a Windows drive or namespace prefix.
///
/// # Parameters
/// * `path`: Path whose first component is inspected.
#[cfg(windows)]
fn has_windows_drive_prefix(path: &Path) -> bool {
    use std::path::Component;
    matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Returns false because non-Windows paths have no Windows drive prefix.
///
/// # Parameters
/// * `_`: Unused path on platforms without Windows drive prefixes.
#[cfg(not(windows))]
fn has_windows_drive_prefix(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs::File;
    #[cfg(unix)]
    use std::io::ErrorKind;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(windows)]
    use std::path::Path;

    #[cfg(windows)]
    use super::resolve_io_path;
    use super::validate_files;
    #[cfg(unix)]
    use super::validate_tee_paths;

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
        let error = validate_files(&[
            (crate::FilePreparationDirection::Stdin, &file),
            (crate::FilePreparationDirection::Stdout, &alias),
        ])
        .expect_err("the same file identity must be rejected");
        assert_eq!(error.source.kind(), std::io::ErrorKind::InvalidInput);
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
        let error = validate_files(&[(crate::FilePreparationDirection::Stdin, &directory)])
            .expect_err("directories are not valid file streams");
        assert_eq!(error.source.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn test_tee_identity_reports_open_permission_failure() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let path = directory.path().join("protected-tee");
        File::create(&path).expect("tee file should be created");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("tee permissions should be restricted");

        let open_result = std::fs::OpenOptions::new().write(true).open(&path);
        let validation = validate_tee_paths(&[], &[(crate::FilePreparationDirection::Stdout, path.as_path())]);
        match open_result {
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                let failure = validation.expect_err("an unopenable regular tee should fail validation");
                assert_eq!(failure.direction, crate::FilePreparationDirection::Stdout);
                assert_eq!(failure.source.kind(), ErrorKind::PermissionDenied);
            }
            Ok(file) => {
                drop(file);
                assert!(
                    validation.is_ok(),
                    "validation should succeed when the current user can open the tee"
                );
            }
            Err(error) => panic!("tee open should fail only when permission is denied: {error}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_tee_identity_reports_parent_metadata_permission_failure() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let protected_directory = directory.path().join("protected");
        std::fs::create_dir(&protected_directory).expect("protected directory should be created");
        let path = protected_directory.join("tee");
        File::create(&path).expect("tee file should be created");
        std::fs::set_permissions(&protected_directory, std::fs::Permissions::from_mode(0o000))
            .expect("directory permissions should be restricted");

        let metadata_result = std::fs::metadata(&path);
        let open_result = std::fs::OpenOptions::new().write(true).open(&path);
        let validation = validate_tee_paths(&[], &[(crate::FilePreparationDirection::Stderr, path.as_path())]);
        std::fs::set_permissions(&protected_directory, std::fs::Permissions::from_mode(0o700))
            .expect("directory permissions should be restored for cleanup");

        match metadata_result {
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                let failure = validation.expect_err("metadata access failure should stop validation");
                assert_eq!(failure.direction, crate::FilePreparationDirection::Stderr);
                assert_eq!(failure.source.kind(), ErrorKind::PermissionDenied);
            }
            Ok(_) => match open_result {
                Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                    let failure = validation.expect_err("an unopenable tee should fail validation");
                    assert_eq!(failure.direction, crate::FilePreparationDirection::Stderr);
                    assert_eq!(failure.source.kind(), ErrorKind::PermissionDenied);
                }
                Ok(file) => {
                    drop(file);
                    assert!(
                        validation.is_ok(),
                        "validation should succeed when the current user can access the tee"
                    );
                }
                Err(error) => panic!("tee open should fail only when permission is denied: {error}"),
            },
            Err(error) => panic!("tee metadata should fail only when permission is denied: {error}"),
        }
    }
}
