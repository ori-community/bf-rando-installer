use crate::settings::LaunchType;
use crate::steam::{get_game_dir, launch_game};
use color_eyre::Result;
use color_eyre::eyre::{Context, bail};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, instrument};

const ORI_DE_APP_ID: &str = "387290";

#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(from = "GameDirS", into = "GameDirS")]
pub struct GameDir {
    pub install: PathBuf,
    pub managed: PathBuf,
}

impl GameDir {
    pub fn new(game_dir: PathBuf) -> Self {
        let mut managed = game_dir.clone();
        managed.extend(["oriDE_Data", "Managed"]);
        Self {
            install: game_dir,
            managed,
        }
    }

    pub fn is_set(&self) -> bool {
        !self.install.as_os_str().is_empty()
    }

    #[instrument(skip(self), fields(?self.install))]
    pub fn launch_game_exe(&self) -> Result<()> {
        opener::open(self.install.join("oriDE.exe")).wrap_err("Error opening game exe")
    }

    #[instrument(skip(self))]
    pub fn try_launch_game(&self, launch_type: LaunchType) -> Result<()> {
        match launch_type {
            LaunchType::Steam => launch_game(ORI_DE_APP_ID),
            LaunchType::File => self.launch_game_exe(),
        }
    }

    #[instrument(skip_all)]
    pub fn launch_game(&self, launch_type: LaunchType) {
        if let Err(err) = self.try_launch_game(launch_type) {
            error!(?err, "Error launching game");
        }
    }
}

impl From<GameDir> for GameDirS {
    fn from(value: GameDir) -> Self {
        let path = value.install.into_os_string();
        match path.into_string() {
            Ok(string) => GameDirS::String(string),
            Err(os_string) => GameDirS::Wide(os_string.encode_wide().collect()),
        }
    }
}

impl From<GameDirS> for GameDir {
    fn from(value: GameDirS) -> Self {
        Self::new(match value {
            GameDirS::String(string) => PathBuf::from(string),
            GameDirS::Wide(wide) => OsString::from_wide(&wide).into(),
        })
    }
}

/// Serialized form of [`GameDir`]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum GameDirS {
    String(String),
    Wide(Vec<u16>),
}

#[instrument(skip(game_dir), fields(game_dir=?game_dir.install))]
pub fn verify_game_dir(game_dir: &GameDir) -> bool {
    if let Err(err) = inner(&game_dir.install) {
        info!(?err, ?game_dir.install, "Failed to validate ori game directory");
        return false;
    }

    return true;

    #[allow(clippy::items_after_statements)]
    fn inner(path: &Path) -> Result<()> {
        let exe_path = path.join("oriDE.exe");
        let metadata = std::fs::metadata(exe_path).wrap_err("Getting exe metadata")?;
        if !metadata.is_file() {
            bail!("Not a file");
        }
        Ok(())
    }
}

#[instrument]
pub fn search_for_game_dir() -> Option<GameDir> {
    match get_game_dir(ORI_DE_APP_ID) {
        Ok(dir) => {
            info!(?dir, "Found ori install dir");

            let game_dir = GameDir::new(dir);
            if verify_game_dir(&game_dir) {
                debug!("Verified ori install dir");
                return Some(game_dir);
            }
        }
        Err(e) => {
            info!(?e, "Failed to find ori install dir");
        }
    }

    None
}
