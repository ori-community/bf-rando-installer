use crate::game::{GameDir, search_for_game_dir, verify_game_dir};
use crate::gui::{AppModal, Inner};
use crate::settings::LaunchType;
use eframe::egui::{Align, ComboBox, Layout, Ui};
use rfd::FileDialog;
use tracing::instrument;

impl Inner {
    #[instrument(skip(self, ui))]
    pub(super) fn draw_settings_ui(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Theme");
                self.settings.theme_preference.radio_buttons(ui);
            });

            self.draw_game_dir_setting(ui);
            self.draw_launch_type_setting(ui);

            Self::draw_show_log_button(ui);
        });
    }

    fn draw_game_dir_setting(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Game installation directory");
            ui.text_edit_singleline(&mut self.settings.game_dir.install.to_string_lossy());
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            self.draw_choose_game_dir_button(ui);
            if ui.button("Auto-Detect").clicked() {
                self.settings.game_dir = search_for_game_dir().unwrap_or_default();
                self.settings.launch_type = LaunchType::Steam;
            }
        });
    }

    fn draw_launch_type_setting(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Game launch type");

            ComboBox::from_id_salt("launch_type_combo")
                .selected_text(self.settings.launch_type.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.settings.launch_type,
                        LaunchType::Steam,
                        LaunchType::Steam.to_string(),
                    );
                    ui.selectable_value(
                        &mut self.settings.launch_type,
                        LaunchType::File,
                        LaunchType::File.to_string(),
                    );
                });
        });
    }

    pub(super) fn draw_choose_game_dir_button(&mut self, ui: &mut Ui) {
        if ui.button("Choose...").clicked() {
            let dir = FileDialog::new().pick_folder();
            if let Some(dir) = dir {
                let game_dir = GameDir::new(dir);
                if verify_game_dir(&game_dir) {
                    self.settings.game_dir = game_dir;
                    self.settings.launch_type = LaunchType::File;
                } else {
                    self.show_invalid_game_dir_modal();
                }
            }
        }
    }

    fn show_invalid_game_dir_modal(&mut self) {
        self.show_modal_ui(AppModal::new().dismissable(true), move |_app, ui, modal| {
            ui.label(
                "The selected directory does not appear to be a valid installation of \
                    Ori and the Blind Forest: Definitive Edition. \
                    Please select another directory.",
            );

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if ui.button("Okay").clicked() {
                    modal.close();
                }
            });
        });
    }
}
