use crate::game::GameDir;
use color_eyre::Result;
use color_eyre::eyre::{Context, ContextCompat};
use eframe::egui::ThemePreference;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, mpsc};
use std::{env, thread};
use tracing::{debug, error, info_span, instrument};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(with = "ThemePreferenceS")]
    pub theme_preference: ThemePreference,
    pub game_dir: GameDir,
    pub launch_type: LaunchType,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum LaunchType {
    Steam,
    File,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_preference: ThemePreference::System,
            game_dir: GameDir::default(),
            launch_type: LaunchType::Steam,
        }
    }
}

impl Settings {
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

impl Display for LaunchType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchType::Steam => f.write_str("Steam"),
            LaunchType::File => f.write_str("File"),
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
