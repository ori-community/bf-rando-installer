use crate::settings::{GogInfo, LaunchType, PathS, Settings};
use crate::windows::{WindowRef, activate_window, drop_file, find_window};
use crate::{gog, steam};
use color_eyre::Result;
use color_eyre::eyre::{Context, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, path};
use tracing::{debug, error, info, instrument, warn};

const STEAM_ORI_DE_APP_ID: &str = "387290";

// All IDs GOG GALAXY recognizes as Ori DE. Taken from my gog galaxy db file.
// I'm not sure what all the IDs are used for.
// As far as I can tell it uses the gog_ ID when the game was installed via GOG,
// and the generic_ ID if it was installed some other way (e.g. steam)
const GOG_ORI_APP_IDS: &[&str] = &[
    "steam_387290",
    "xboxone_559544937",
    "gog_1384944984",
    "winstore_9nblggh1z6fb",
    "nswitch_010061D00DB74000",
    "itch_Ori and the Blind Forest - Definitive Edition [FitGirl Repack]",
    "itch_28eee1d8120b1c6c9c5821c0df585afa369e66d5a1ab4257edff3495",
    "humble_GAME_FROM_DATA_FILE-887-20200411074355",
    "origin_a2fa6b53718de50661c436eed98720bec37b6afc",
    "origin_29729da8b7ef7caa2bd0c1fd613fc94efb071ab000a2cfdf1d4530019028fd1f",
    "beamdog_951cdb72-02f2-4dc2-a077-24ab49631e12",
    "minecraft_951cdb72-02f2-4dc2-a077-24ab49631e12",
    "minecraft_aa86dbf7-dc7a-42a8-87a0-fe9729bf6fb4",
    "minecraft_9a055e97-c51b-4e28-80a6-6da044c46a88",
    "discord_8f5ff3ab-9fbc-4659-8f40-ee2be7f4a7c7",
    "origin_44269",
    "test_/ori-and-the-blind-forest-free-download/",
    "test_Ori and The Blind Forest - Definitive Edition",
    "atari_Ori and The Blind Forest - Definitive Edition",
    "test_b62bd09bec4e079079403b88fb1cc3271a0d4b03",
    "generic_51153454636430882",
];

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
        debug!("Starting Ori exe");
        opener::open(self.install.join("oriDE.exe")).wrap_err("Error opening game exe")
    }

    #[instrument(skip(self, settings))]
    pub fn try_launch_game(&self, settings: &Settings) -> Result<()> {
        match settings.launch_type {
            LaunchType::Steam => steam::launch_game(STEAM_ORI_DE_APP_ID),
            LaunchType::Direct => self.launch_game_exe(),
            LaunchType::GogGalaxy => match &settings.gog {
                Some(gog) => gog::launch_game(&gog.galaxy, &gog.game_id),
                None => {
                    error!(
                        "Launch type is set to GOG GALAXY, but no GOG was configured in settings. Launching as Direct instead."
                    );
                    self.launch_game_exe()
                }
            },
        }
    }

    #[instrument(skip_all)]
    pub fn launch_game(&self, settings: &Settings) {
        info!(?settings.launch_type, "Launching game");
        if let Err(err) = self.try_launch_game(settings) {
            error!(?err, "Error launching game");
        }
    }

    #[instrument(skip_all)]
    pub fn try_play_seed(&self, seed_path: &Path, settings: &Settings) -> Result<()> {
        self.drop_game_file(seed_path, DropMode::DropOrStart, settings)
    }

    #[instrument(skip_all)]
    pub fn try_play_practice_file(
        &self,
        practice_file_path: &Path,
        settings: &Settings,
    ) -> Result<()> {
        self.drop_game_file(practice_file_path, DropMode::DropAndStart, settings)
    }

    #[instrument(skip_all, fields(file))]
    pub fn drop_game_file(
        &self,
        file: &Path,
        drop_mode: DropMode,
        settings: &Settings,
    ) -> Result<()> {
        let file = path::absolute(file).unwrap_or_else(|err| {
            error!(?err, "Failed to make path absolute. Using original path.");
            file.to_owned()
        });

        if try_drop_onto_game_window(&file)? {
            return Ok(());
        }

        info!("No window to drop file onto, starting the game");
        match drop_mode {
            DropMode::DropOrStart => (),
            DropMode::DropAndStart => {
                info!("Writing startup file to drop file on startup");
                if let Err(err) = self
                    .write_startup_file(StartupAction::DropFile(&file.to_string_lossy()))
                    .wrap_err("Writing startup file")
                {
                    error!(?err, "Error writing startup file");
                }
            }
        }

        self.try_launch_game(settings).wrap_err("Launching game")
    }

    #[instrument]
    fn write_startup_file(&self, action: StartupAction) -> Result<()> {
        let line = match action {
            StartupAction::DropFile(path) => format!("dropfile:{path}"),
        };

        fs::write(
            self.install
                .join(".SafeToDelete.RandomizerOneTimeStartupInfo.txt"),
            line,
        )
        .wrap_err("Writing startup file")
    }
}

#[instrument]
fn try_drop_onto_game_window(file: &Path) -> Result<bool> {
    if let Some(window) = find_game_window() {
        info!("Dropping file onto game window");
        drop_file(window, &file).wrap_err("Dropping file")?;
        activate_window(window);
        Ok(true)
    } else {
        Ok(false)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum DropMode {
    DropOrStart,
    DropAndStart,
}

#[derive(Debug)]
pub enum StartupAction<'a> {
    DropFile(&'a str),
}

impl From<GameDir> for GameDirS {
    fn from(value: GameDir) -> Self {
        Self(value.install.into())
    }
}

impl From<GameDirS> for GameDir {
    fn from(value: GameDirS) -> Self {
        Self::new(value.0.into())
    }
}

/// Serialized form of [`GameDir`]
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct GameDirS(PathS);

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
pub fn search_for_game_dir() -> Option<DetectedGameDir> {
    match steam::get_game_dir(STEAM_ORI_DE_APP_ID) {
        Ok(dir) => {
            info!(?dir, "Found steam ori install dir");

            let game_dir = GameDir::new(dir);
            if verify_game_dir(&game_dir) {
                debug!("Verified ori install dir");
                return Some(DetectedGameDir::Steam { game_dir });
            }
        }
        Err(e) => {
            info!(?e, "Failed to find steam ori install dir");
        }
    }

    search_for_gog_game().map(DetectedGameDir::Gog)
}

#[instrument]
pub fn search_for_gog_game() -> Option<GogGameDir> {
    info!("Searching for gog game install");
    match gog::get_game(GOG_ORI_APP_IDS) {
        Ok(gog_game) => {
            info!(?gog_game, "Found GOG Ori install");

            let game_dir = GameDir::new(gog_game.exe_dir);
            verify_game_dir(&game_dir).then(|| {
                debug!("Verified ori install dir");

                let gog = match gog::find_galaxy() {
                    Ok(galaxy) => Some(GogInfo {
                        game_id: gog_game.id,
                        galaxy,
                    }),
                    Err(err) => {
                        warn!(?err, "Didn't find GOG GALAXY install");
                        None
                    }
                };

                GogGameDir { game_dir, gog }
            })
        }
        Err(e) => {
            info!(?e, "Failed to find GOG ori install dir");
            None
        }
    }
}

pub enum DetectedGameDir {
    Steam { game_dir: GameDir },
    Gog(GogGameDir),
}

pub struct GogGameDir {
    pub game_dir: GameDir,
    pub gog: Option<GogInfo>,
}

fn find_game_window() -> Option<WindowRef> {
    find_window(
        Some(c"UnityWndClass"),
        Some(c"Ori And The Blind Forest: Definitive Edition"),
    )
}
