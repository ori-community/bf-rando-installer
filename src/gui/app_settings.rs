use crate::game::{GameDir, search_for_game_dir, verify_game_dir};
use crate::gui::{AppModal, Inner};
use crate::settings::{LaunchType, MoveSeedMode};
use crate::url_handler::{ensure_url_handler_exists, is_url_handler_set, remove_url_handler};
use eframe::egui::{Align, ComboBox, Layout, Ui};
use rfd::FileDialog;
use std::fmt::Display;
use tracing::{error, instrument};

impl Inner {
    #[instrument(skip(self, ui))]
    pub(super) fn draw_settings_ui(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
        });

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Theme");
                self.settings.theme_preference.radio_buttons(ui);
            });

            self.draw_game_dir_setting(ui);
            self.draw_launch_type_setting(ui);
            self.draw_seed_move_setting(ui);

            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.settings.self_update, "Auto-Update");
            });

            ui.horizontal_wrapped(|ui| {
                let old_url = self.settings.set_url_handler;
                ui.checkbox(&mut self.settings.set_url_handler, "URL Handler");

                if !old_url && self.settings.set_url_handler {
                    if let Err(err) = ensure_url_handler_exists() {
                        error!(?err, "Couldn't register URL handler");
                        self.settings.set_url_handler = false;
                        self.push_error("Failed to register URL Handler");
                    }
                }

                if !self.settings.set_url_handler
                    && let Ok(true) = is_url_handler_set()
                {
                    if ui.button("Unset").clicked() {
                        if let Err(err) = remove_url_handler() {
                            error!(?err, "Couldn't unset URL handler");
                            self.push_error("Failed to unset URL Handler");
                        }
                    }
                }
            });

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
            combo_box(
                ui,
                "Game launch type",
                &mut self.settings.launch_type,
                &[LaunchType::Steam, LaunchType::File],
            );
        });
    }

    fn draw_seed_move_setting(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            combo_box(
                ui,
                "Move seed file",
                &mut self.settings.move_seed_mode,
                &[
                    MoveSeedMode::Always,
                    MoveSeedMode::Never,
                    MoveSeedMode::Auto,
                ],
            );
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

fn combo_box<T: Display + Clone + PartialEq>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[T],
) {
    ComboBox::from_label(label)
        .selected_text(value.to_string())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(value, option.clone(), option.to_string());
            }
        });
}
