use crate::files::{recover_file, safer_file_write};
use crate::game::{DetectedGameDir, GameDir, GogGameDir, search_for_game_dir};
use color_eyre::Result;
use color_eyre::eyre::{Context, ContextCompat, eyre};
use eframe::egui::ThemePreference;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, mpsc};
use std::{env, thread};
use tracing::{debug, error, info_span, instrument};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(clippy::struct_excessive_bools)]
pub struct Settings {
    #[serde(with = "ThemePreferenceS")]
    pub theme_preference: ThemePreference,
    pub game_dir: GameDir,
    pub launch_type: LaunchType,
    pub gog: Option<GogInfo>,
    pub move_seed_mode: MoveSeedMode,
    pub self_update: bool,
    pub stay_on_latest: bool,
    pub set_url_handler: bool,
    pub set_file_association: bool,
    pub network: NetworkSettings,
    pub show_beta: bool,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum LaunchType {
    Steam,
    #[serde(alias = "File")]
    Direct,
    GogGalaxy,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GogInfo {
    pub game_id: String,
    #[serde(with = "PathS")]
    pub galaxy: PathBuf,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum MoveSeedMode {
    Always,
    Never,
    Auto,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkSettings {
    pub offline_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_preference: ThemePreference::System,
            game_dir: GameDir::default(),
            launch_type: LaunchType::Steam,
            gog: None,
            move_seed_mode: MoveSeedMode::Auto,
            self_update: true,
            stay_on_latest: false,
            set_url_handler: true,
            set_file_association: true,
            network: NetworkSettings::default(),
            show_beta: false,
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let local_appdata = env::var_os("LOCALAPPDATA").wrap_err("Error retrieving %LOCALAPPDATA%")?;

    let mut settings_path = PathBuf::from(local_appdata);
    settings_path.push("Ori DE Randomizer");

    Ok(settings_path)
}

impl Settings {
    fn save_path() -> Result<PathBuf> {
        config_dir().map(|mut p| {
            p.push("settings.toml");
            p
        })
    }

    #[instrument]
    fn try_load() -> Result<Self> {
        let path = Self::save_path()?;
        recover_file(&path).wrap_err("Recovering settings file")?;
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

        safer_file_write(path, contents).wrap_err("Writing settings file")?;

        Ok(())
    }

    #[instrument]
    pub fn load() -> Self {
        let settings = Self::try_load().unwrap_or_else(|err| {
            error!(?err, "Error loading settings");
            Settings::default()
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
            error!("Could not save settings async (save thread died). Saving sync.");
            self.save();
        }
    }

    fn start_save_thread() -> Sender<Settings> {
        let (tx, rx) = mpsc::channel::<Settings>();
        thread::spawn(move || {
            let _span = info_span!("save_thread").entered();
            while let Ok(mut settings) = rx.recv() {
                while let Ok(new_settings) = rx.try_recv() {
                    settings = new_settings;
                }

                settings.save();
            }
        });
        tx
    }

    #[instrument(skip_all)]
    pub fn auto_detect_game_dir(&mut self) {
        match search_for_game_dir() {
            None => {
                self.game_dir = GameDir::default();
                self.launch_type = LaunchType::Direct;
            }
            Some(detected) => match detected {
                DetectedGameDir::Steam { game_dir } => {
                    self.game_dir = game_dir;
                    self.launch_type = LaunchType::Steam;
                }
                DetectedGameDir::Gog(GogGameDir { game_dir, gog }) => {
                    self.game_dir = game_dir;
                    if let Some(gog) = gog {
                        self.gog = Some(gog);
                        self.launch_type = LaunchType::GogGalaxy;
                    } else {
                        self.launch_type = LaunchType::Direct;
                    }
                }
            },
        }
    }
}

impl NetworkSettings {
    pub fn check_offline_mode(self) -> Result<()> {
        if self.offline_mode {
            Err(eyre!("Can't perform network request in offline mode"))
        } else {
            Ok(())
        }
    }
}

impl Display for LaunchType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchType::Steam => f.write_str("Steam"),
            LaunchType::Direct => f.write_str("Direct"),
            LaunchType::GogGalaxy => f.write_str("GOG GALAXY"),
        }
    }
}

impl Display for MoveSeedMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MoveSeedMode::Always => f.write_str("Always"),
            MoveSeedMode::Never => f.write_str("Never"),
            MoveSeedMode::Auto => f.write_str("Auto"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ThemePreference")]
enum ThemePreferenceS {
    Dark,
    Light,
    System,
}

/// Serialized form of a [`PathBuf`]
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathS {
    String(String),
    Wide(Vec<u16>),
}

impl PathS {
    fn serialize<'a, T, S: Serializer>(value: &'a T, serializer: S) -> Result<S::Ok, S::Error>
    where
        &'a T: Into<Self>,
    {
        <Self as Serialize>::serialize(&value.into(), serializer)
    }

    fn deserialize<'de, T, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error>
    where
        Self: Into<T>,
    {
        <Self as Deserialize>::deserialize(deserializer).map(Self::into)
    }
}

impl From<PathS> for PathBuf {
    fn from(value: PathS) -> Self {
        match value {
            PathS::String(string) => PathBuf::from(string),
            PathS::Wide(wide) => OsString::from_wide(&wide).into(),
        }
    }
}

impl From<PathBuf> for PathS {
    fn from(value: PathBuf) -> PathS {
        let path = value.into_os_string();
        match path.into_string() {
            Ok(string) => PathS::String(string),
            Err(os_string) => PathS::Wide(os_string.encode_wide().collect()),
        }
    }
}

impl From<&PathBuf> for PathS {
    fn from(value: &PathBuf) -> PathS {
        value.clone().into()
    }
}
