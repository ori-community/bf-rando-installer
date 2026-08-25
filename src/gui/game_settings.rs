use crate::files::is_file;
use crate::gui::{Inner, open_file_button};
use crate::utils::CachedValue;
use color_eyre::eyre::WrapErr;
use eframe::egui::Ui;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tracing::{error, instrument};

#[derive(Debug)]
pub struct GameSettings {
    debug_mode_enabled: CachedValue<bool, ()>,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            debug_mode_enabled: CachedValue::new(check_debug_mode_enabled),
        }
    }
}

static DEBUG_MODE_FILE: LazyLock<&Path> = LazyLock::new(|| Path::new(r"C:\temp\moonDebugPC.txt"));

#[instrument(skip_all)]
fn check_debug_mode_enabled((): ()) -> bool {
    is_file(*DEBUG_MODE_FILE).unwrap_or_else(|err| {
        error!(?err, "Error checking if debug mode is enabled");
        false
    })
}

impl Inner {
    #[instrument(skip_all)]
    pub(super) fn draw_game_settings_ui(&mut self, ui: &mut Ui) {
        ui.separator();
        self.draw_open_files(ui);
        ui.horizontal_wrapped(|ui| {
            let debug = *self.game_settings.debug_mode_enabled.get_cached(());
            let mut new_debug = debug;
            ui.checkbox(&mut new_debug, "Enable Debug Mode")
                .on_hover_text("Enable In-Game Debug Menu");
            if new_debug != debug {
                self.set_debug_mode(new_debug);
                self.game_settings.debug_mode_enabled.update(());
            }
        });
    }

    #[instrument(skip_all)]
    fn draw_open_files(&self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            open_file_button(ui, "Rando Settings", || {
                self.rando_install_path("RandomizerSettings.txt")
            });
            open_file_button(ui, "Random Exp Names", || {
                self.rando_install_path("ExpNames.txt")
            });
        });
        ui.horizontal_wrapped(|ui| {
            open_file_button(ui, "Controls (Rando)", || {
                self.rando_install_path("RandomizerRebinding.txt")
            });
            open_file_button(ui, "Controls (KBM)", || game_app_path("KeyRebindings.txt"));
            open_file_button(ui, "Controls (Controller)", || {
                game_app_path("ControllerRebindings.txt")
            });
            open_file_button(ui, "Controller Remaps", || {
                game_app_path("ControllerButtonRemaps.txt")
            });
        });
    }
}

impl Inner {
    #[instrument(skip(self))]
    fn set_debug_mode(&mut self, enabled: bool) {
        let result = if enabled {
            match std::fs::File::options()
                .create_new(true)
                .write(true)
                .open(*DEBUG_MODE_FILE)
            {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
                Err(err) => Err(err).wrap_err("Writing debug mode file"),
            }
        } else {
            match std::fs::remove_file(*DEBUG_MODE_FILE) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err).wrap_err("Deleting debug mode file"),
            }
        };

        if let Err(err) = result {
            error!(?err, "Error setting debug mode");
            self.push_error("Error setting debug mode");
        }
    }

    fn rando_install_path(&self, file: &str) -> PathBuf {
        self.settings.game_dir.install.join(file)
    }
}

fn game_app_path(file: &str) -> PathBuf {
    let Some(local_appdata) = env::var_os("LOCALAPPDATA") else {
        return PathBuf::new();
    };
    let mut path = PathBuf::from(local_appdata);
    path.extend(["Ori and the Blind Forest DE", file]);
    path
}
