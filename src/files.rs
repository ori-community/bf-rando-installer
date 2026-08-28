use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{error, info, instrument};

#[instrument]
pub fn move_file(from: &PathBuf, to: &PathBuf) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => (),
        Err(err) => return Err(err).wrap_err("Renaming file"),
    }

    // Paths are on different file systems, so move by copy + delete
    std::fs::copy(from, to).wrap_err("Copying file")?;
    if let Err(err) = std::fs::remove_file(from) {
        error!(
            ?err,
            "Could not delete old file after copying it to target."
        );
    }

    Ok(())
}

/// Checks if the file at `path` exists and is a file.
pub fn is_file(path: &Path) -> Result<bool, io::Error> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.is_file()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn make_valid_filename(name: &str) -> String {
    static INVALID_CHARS: [char; 10] = ['/', '\0', '\\', ':', '*', '?', '"', '<', '>', '|'];

    name.replace(INVALID_CHARS, "_")
}

/// "Recovers" a file that was written using [`safer_file_write`]:
/// - If `<path>~` exists, it's deleted.
/// - If `path` and `<path>.old~` exist, the latter is deleted.
/// - If `path` does not and `<path>.old~` does exist, the latter is renamed to the former.
pub fn recover_file(path: impl AsRef<Path>) -> Result<()> {
    #[instrument]
    fn recover_file(path: &Path) -> Result<()> {
        let tmp_path = tmp_path(path);
        if is_file(&tmp_path).wrap_err("Check tmp file")? {
            info!(?tmp_path, "Deleting tmp file");
            std::fs::remove_file(tmp_path).wrap_err("Removing tmp file")?;
        }

        let backup_path = backup_path(path);
        if is_file(&backup_path).wrap_err("Checking backup file")? {
            if is_file(path).wrap_err("Checking file")? {
                info!(?backup_path, "Deleting backup file");
                std::fs::remove_file(backup_path).wrap_err("Deleting backup file")?;
            } else {
                info!(?backup_path, ?path, "Restoring file from backup");
                std::fs::rename(backup_path, path).wrap_err("Restoring backup file")?;
            }
        }

        Ok(())
    }

    recover_file(path.as_ref())
}

/// Writes a file in multiple steps to minimize the chance of a corrupted or missing file in case of error.
///
/// Procedure is as follows:
/// 1. Write contents to `<path>~`
/// 1. fsync `<path>~`
/// 1. Rename `<path>` to `<path>.old~`
/// 1. Rename `<path>~` to `<path>`
/// 1. Delete `<path>.old~`
///
/// Should that process be interrupted, a valid file can always be recovered via [`recover_file`].
pub fn safer_file_write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
    #[instrument(skip(contents))]
    fn safer_file_write(path: &Path, contents: &[u8]) -> Result<()> {
        let tmp_path = tmp_path(path);
        let backup_path = backup_path(path);

        std::fs::write(&tmp_path, contents).wrap_err("Writing (tmp) file")?;
        fsync_file(&tmp_path).wrap_err("Flushing tmp file")?;
        match std::fs::rename(path, &backup_path) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e).wrap_err("Creating backup"),
        }
        std::fs::rename(tmp_path, path).wrap_err("Replacing file")?;
        match std::fs::remove_file(backup_path) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e).wrap_err("Deleting backup"),
        }
        Ok(())
    }

    safer_file_write(path.as_ref(), contents.as_ref())
}

#[instrument]
fn fsync_file(path: &Path) -> Result<()> {
    std::fs::File::options()
        .write(true)
        .open(path)
        .wrap_err("Opening file")?
        .sync_all()
        .wrap_err("Flushing file")?;
    Ok(())
}

fn tmp_path(path: impl Into<PathBuf>) -> PathBuf {
    let mut path = path.into().into_os_string();
    path.push("~");
    path.into()
}

fn backup_path(path: impl Into<PathBuf>) -> PathBuf {
    let mut path = path.into().into_os_string();
    path.push(".old~");
    path.into()
}
