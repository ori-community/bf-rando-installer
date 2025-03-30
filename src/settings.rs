use crate::steam::get_game_dir;
use color_eyre::Result;
use color_eyre::eyre::{Context, ContextCompat, bail};
use eframe::egui::ThemePreference;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, mpsc};
use std::{env, thread};
use tracing::{debug, error, info, instrument};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(with = "ThemePreferenceS")]
    pub theme_preference: ThemePreference,
    pub game_dir: GameDir,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_preference: ThemePreference::System,
            game_dir: Default::default(),
        }
    }
}

impl Settings {
    #[instrument]
    pub fn load() -> Self {
        let settings = Self::try_load().unwrap_or_else(|err| {
            error!(?err, "Error loading settings");
            Default::default()
        });

        debug!(?settings, "Loaded settings");

        settings
    }

    #[instrument(skip(self))]
    pub fn save(&self) -> bool {
        match self.try_save() {
            Ok(()) => {
                debug!(settings=?self, "Saved settings");
                true
            }
            Err(err) => {
                error!(?err, "Error saving settings");
                false
            }
        }
    }

    /// Save on a background thread.
    /// The background thread will serialize the saves.
    #[instrument(skip(self))]
    pub fn save_async(&self) {
        static SAVE_CHANNEL: LazyLock<Sender<Settings>> =
            LazyLock::new(Settings::start_save_thread);
        if SAVE_CHANNEL.send(self.clone()).is_err() {
            error!("Could not save settings (save thread died). Saving sync.");
            self.save();
        }
    }

    fn start_save_thread() -> Sender<Settings> {
        let (tx, rx) = mpsc::channel::<Settings>();
        thread::spawn(move || {
            loop {
                let Ok(mut settings) = rx.recv() else { break };

                while let Ok(new_settings) = rx.try_recv() {
                    settings = new_settings;
                }

                settings.save();
            }
        });
        tx
    }
}

impl Settings {
    fn save_path() -> Result<PathBuf> {
        let local_appdata =
            env::var_os("LOCALAPPDATA").wrap_err("Error retrieving %LOCALAPPDATA%")?;

        let mut settings_path = PathBuf::from(local_appdata);
        settings_path.extend(["ori-rando-installer", "settings.toml"]);

        Ok(settings_path)
    }

    #[instrument]
    fn try_load() -> Result<Self> {
        let path = Self::save_path()?;
        let contents = std::fs::read_to_string(path).wrap_err("Error reading settings file")?;
        let settings = toml::from_str(&contents).wrap_err("Error parsing settings")?;

        debug!(?settings, "Loaded settings");

        Ok(settings)
    }

    #[instrument(skip(self))]
    fn try_save(&self) -> Result<()> {
        let contents = toml::to_string(self).wrap_err("Error serializing settings")?;

        let path = Self::save_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).wrap_err("Error creating settings directory")?;
        }
        std::fs::write(path, contents).wrap_err("Error writing settings")?;

        Ok(())
    }
}

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
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ThemePreference")]
enum ThemePreferenceS {
    Dark,
    Light,
    System,
}

/// Serialized form of [`GameDir`]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum GameDirS {
    String(String),
    Wide(Vec<u16>),
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

#[instrument(skip(game_dir), fields(game_dir=?game_dir.install))]
pub fn verify_game_dir(game_dir: &GameDir) -> bool {
    if let Err(err) = inner(&game_dir.install) {
        info!(?err, ?game_dir.install, "Failed to validate ori game directory");
        return false;
    }

    return true;

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
    match get_game_dir("387290") {
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
