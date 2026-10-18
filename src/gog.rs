use crate::windows::get_protocol_handler_exe;
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use rusqlite::types::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use tracing::{debug, instrument};

#[derive(Debug)]
pub struct GogGame {
    pub id: String,
    pub exe_dir: PathBuf,
}

#[instrument(skip_all, fields(first_game_id=game_ids.first()))]
pub fn get_game(game_ids: &[&str]) -> Result<GogGame> {
    debug!("Trying to find gog game dir");
    let db = rusqlite::Connection::open(r"C:\ProgramData\GOG.com\Galaxy\storage\galaxy-2.0.db")
        .wrap_err("Opening gog galaxy db")?;

    rusqlite::vtab::array::load_module(&db).wrap_err("Loading rarray sqlite extension")?;

    let game_ids = Rc::new(
        game_ids
            .iter()
            .map(|&v| Value::from(v.to_owned()))
            .collect::<Vec<_>>(),
    );

    let mut stmt = db
        .prepare(
            r"
SELECT
    playTask.gameReleaseKey,
    launchParams.executablePath
FROM PlayTasks AS playTask
JOIN PlayTaskLaunchParameters AS launchParams
    ON launchParams.playTaskId = playTask.id
WHERE
    playTask.gameReleaseKey IN rarray(?1)
    AND playTask.isPrimary
;",
        )
        .wrap_err("preparing sql")?;

    let (id, exe_path) = stmt
        .query_one([game_ids], |row| {
            Ok((
                row.get::<_, String>("gameReleaseKey")?,
                row.get::<_, String>("executablePath")?,
            ))
        })
        .wrap_err("querying db for executable path")?;

    let exe_dir = Path::new(&exe_path)
        .parent()
        .ok_or_else(|| eyre!("Game executable path doesn't have a parent: {exe_path:?}"))?;

    debug!(game_id=%id, ?exe_path, "Found gog game");

    Ok(GogGame {
        id,
        exe_dir: exe_dir.to_owned(),
    })
}

#[instrument]
pub fn find_galaxy() -> Result<PathBuf> {
    debug!("Searching for gog install");
    get_protocol_handler_exe("goggalaxy").map(|path| {
        debug!(?path, "Retrieved gog galaxy install path");
        PathBuf::from(path)
    })
}

#[instrument]
pub fn launch_game(galaxy: &Path, game_id: &str) -> Result<()> {
    debug!("Launching gog game");
    Command::new(galaxy)
        .args(["/command=runGame", &format!("/gameId={game_id}")])
        .spawn()
        .wrap_err("Launching galaxy")?;
    Ok(())
}
