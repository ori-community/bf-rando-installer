use crate::dll_management::{OriDllKind, install_dll};
use crate::game::{GameDir, search_for_game_dir, verify_game_dir};
use crate::gui::{AppModal, Inner};
use crate::settings::{LaunchType, MoveSeedMode};
use crate::windows::{
    AssociationKind, ensure_association_exists, is_association_set, remove_association,
};
use color_eyre::Result;
use color_eyre::eyre::Context;
use eframe::egui::{Align, Color32, ComboBox, Layout, RichText, Sides, Ui};
use rfd::FileDialog;
use std::env;
use std::fmt::Display;
use std::process::{Command, Stdio};
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

            self.draw_url_handler_setting(ui);
            self.draw_file_association_setting(ui);

            ui.checkbox(&mut self.settings.network.offline_mode, "Offline Mode")
                .on_hover_text("Disable/Forbid all network requests");

            self.draw_uninstall_button(ui);

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
                    MoveSeedMode::Auto,
                    MoveSeedMode::Always,
                    MoveSeedMode::Never,
                ],
            );
        });
    }

    #[instrument(skip_all)]
    fn draw_url_handler_setting(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            let old_url = self.settings.set_url_handler;
            ui.checkbox(&mut self.settings.set_url_handler, "URL Handler");

            if !old_url && self.settings.set_url_handler {
                if let Err(err) = ensure_association_exists(AssociationKind::Url) {
                    error!(?err, "Couldn't register URL handler");
                    self.settings.set_url_handler = false;
                    self.push_error("Failed to register URL Handler");
                }
            }

            if !self.settings.set_url_handler && old_url {
                if let Err(err) = remove_association(AssociationKind::Url) {
                    error!(?err, "Couldn't unset URL handler");
                    self.push_error("Failed to unset URL Handler");
                }
            }

            if !self.settings.set_url_handler
                && let Ok(true) = is_association_set(AssociationKind::Url)
                && ui.button("Unset").clicked()
            {
                if let Err(err) = remove_association(AssociationKind::Url) {
                    error!(?err, "Couldn't unset URL handler");
                    self.push_error("Failed to unset URL Handler");
                }
            }
        });
    }

    #[instrument(skip_all)]
    fn draw_file_association_setting(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            let old_association = self.settings.set_file_association;
            ui.checkbox(&mut self.settings.set_file_association, "File Association");

            if !old_association && self.settings.set_file_association {
                if let Err(err) = ensure_association_exists(AssociationKind::File) {
                    error!(?err, "Couldn't register file association");
                    self.settings.set_file_association = false;
                    self.push_error("Failed to set File Association");
                }
            }

            if !self.settings.set_file_association && old_association {
                if let Err(err) = remove_association(AssociationKind::File) {
                    error!(?err, "Couldn't unset file association");
                    self.push_error("Failed to unset File Association");
                }
            }

            if !self.settings.set_file_association
                && let Ok(true) = is_association_set(AssociationKind::File)
                && ui.button("Unset").clicked()
            {
                if let Err(err) = remove_association(AssociationKind::File) {
                    error!(?err, "Couldn't unset file association");
                    self.push_error("Failed to unset File Association");
                }
            }
        });
    }

    fn draw_uninstall_button(&mut self, ui: &mut Ui) {
        if ui
            .button(RichText::new("Uninstall...").color(Color32::RED))
            .clicked()
        {
            let mut uninstaller = Uninstaller::default();
            self.show_modal_ui(
                AppModal::new().dismissable(true),
                move |inner, ui, modal| uninstaller.draw(inner, ui, modal),
            );
        }
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

pub(super) struct Uninstaller {
    delete_settings: bool,
    uninstall_rando: bool,
}

impl Default for Uninstaller {
    fn default() -> Self {
        Self {
            delete_settings: true,
            uninstall_rando: true,
        }
    }
}

impl Uninstaller {
    fn draw(&mut self, inner: &mut Inner, ui: &mut Ui, modal: &mut AppModal) {
        ui.heading("Uninstall");

        ui.label("Removes file associations and additional files created by this program.");

        ui.checkbox(&mut self.delete_settings, "Delete app settings");

        let (can_uninstall_rando, reason) = if matches!(&inner.current_dll, Some(dll) if dll.kind == OriDllKind::Vanilla)
        {
            (false, " (already vanilla)")
        } else if inner
            .all_dlls
            .iter()
            .all(|dll| dll.kind != OriDllKind::Vanilla)
        {
            (false, " (no Vanilla DLL found)")
        } else {
            (true, "")
        };

        self.uninstall_rando &= can_uninstall_rando;
        ui.add_enabled_ui(can_uninstall_rando, |ui| {
            ui.checkbox(
                &mut self.uninstall_rando,
                format!("Return game to vanilla{reason}"),
            );
        });

        let (confirm, cancel) = Sides::new().show(
            ui,
            |ui| {
                ui.button(RichText::new("Uninstall").color(Color32::RED))
                    .clicked()
            },
            |ui| ui.button("Cancel").clicked(),
        );

        if confirm {
            self.uninstall(inner);
            if inner.error_messages.is_empty() {
                std::process::exit(0);
            }
        }

        if confirm || cancel {
            modal.close();
        }
    }

    #[instrument(skip_all)]
    fn uninstall(&self, inner: &mut Inner) {
        if let Err(err) = remove_association(AssociationKind::Url) {
            error!(?err, "Couldn't unset url handler");
            inner.push_error("Failed to unset URL Handler");
        }

        if let Err(err) = remove_association(AssociationKind::File) {
            error!(?err, "Couldn't unset file association");
            inner.push_error("Failed to unset File Association");
        }

        if self.uninstall_rando {
            if let Some(vanilla_dll) = inner
                .all_dlls
                .iter()
                .find(|&dll| dll.kind == OriDllKind::Vanilla)
            {
                if let Err(err) =
                    install_dll(&inner.settings.game_dir, vanilla_dll, &inner.all_dlls)
                {
                    error!(?err, "Couldn't revert to Vanilla");
                    inner.push_error("Failed to revert to vanilla DLL");
                }
            } else {
                error!(?inner.all_dlls, "Cannot revert to Vanilla, because no vanilla DLL was found");
                inner.push_error("Cannot revert to Vanilla, because no vanilla DLL was found");
            }
        }

        if self.delete_settings {
            if let Err(err) = Self::start_delete_settings() {
                error!(?err, "Failed to start process to delete settings");
                inner.push_error("Failed to delete settings");
            }
        }

        std::process::exit(0);
    }

    #[instrument(skip_all)]
    fn start_delete_settings() -> Result<()> {
        let self_file = env::current_exe().wrap_err("Getting current exe")?;

        Command::new(self_file)
            .arg("--delete-everything")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("Failed to spawn replacement process")?;

        std::process::exit(0);
    }
}
