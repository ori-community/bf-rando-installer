use color_eyre::Result;
use color_eyre::eyre::Context;
use std::io;
use std::path::PathBuf;
use tracing::{error, instrument};

#[instrument]
pub fn move_file(from: &PathBuf, to: &PathBuf) -> Result<()> {
    match std::fs::rename(&from, &to) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => (),
        Err(err) => return Err(err).wrap_err("Renaming randomizer.dat"),
    }

    // Paths are on different file systems, so move by copy + delete
    std::fs::copy(from, to).wrap_err("Copying randomizer.dat")?;
    if let Err(err) = std::fs::remove_file(from) {
        error!(
            ?err,
            "Could not delete old randomizer.dat after copying it to target."
        );
    }

    Ok(())
}

pub fn make_valid_filename(name: &str) -> String {
    static INVALID_CHARS: [char; 10] = ['/', '\0', '\\', ':', '*', '?', '"', '<', '>', '|'];

    name.replace(INVALID_CHARS, "_")
}
