use crate::gui::{Inner, open_file_button};
use eframe::egui::Ui;
use std::env;
use std::path::PathBuf;
use tracing::instrument;

impl Inner {
    pub(super) fn draw_game_settings_ui(&mut self, ui: &mut Ui) {
        ui.separator();
        self.draw_open_files(ui);
    }

    #[instrument(skip_all)]
    fn draw_open_files(&self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Open settings:");
            open_file_button(ui, "Randomizer", || {
                self.rando_install_path("RandomizerSettings.txt")
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Open Controls:");
            open_file_button(ui, "Rando", || {
                self.rando_install_path("RandomizerRebinding.txt")
            });
            open_file_button(ui, "Vanilla (KBM)", || game_app_path("KeyRebindings.txt"));
            open_file_button(ui, "Vanilla (Controller)", || {
                game_app_path("ControllerRebindings.txt")
            });
            open_file_button(ui, "Controller Remaps", || {
                game_app_path("ControllerButtonRemaps.txt")
            });
        });
    }
}

impl Inner {
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
