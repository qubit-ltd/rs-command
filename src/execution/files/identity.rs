use std::fs::File;
use std::io;
use std::path::Path;

use same_file::Handle;

/// Checks ordinary-file status and identity using the opened handles themselves.
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

/// Resolves a relative path against a captured working directory lexically.
#[allow(dead_code)]
pub(super) fn absolute_lexical_path(base: &Path, path: &Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::validate_files;

    #[test]
    fn identity_uses_open_handles_and_detects_cloned_file_aliases() {
        let file = tempfile::tempfile().expect("temporary file should be created");
        let alias = file.try_clone().expect("file handle should be cloned");
        let error = validate_files(&[("stdin", &file), ("stdout", &alias)])
            .expect_err("the same file identity must be rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn identity_rejects_non_regular_handles() {
        let directory = File::open(std::env::temp_dir()).expect("temp directory should open");
        let error = validate_files(&[("stdin", &directory)])
            .expect_err("directories are not valid file streams");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
