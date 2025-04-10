use crate::dll_classifier::RandoVersion;
use crate::dll_management::install_new_dll;
use crate::gui::{Inner, InstalledState, NewestState};
use crate::orirando::download_dll;
use eframe::egui::{Align, Color32, FontFamily, FontId, Layout, Spinner, TextStyle, Ui, Widget};
use egui_alignments::Aligner;
use tracing::{error, info, instrument, warn};

impl Inner {
    pub(super) fn draw_rando_version(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| match self.newest_version_installed {
            InstalledState::Unknown => {}
            InstalledState::Checking => {
                ui.label("Loading installed versions...");
            }
            InstalledState::None => {
                self.draw_install_button(ui, "Install Randomizer", true);
            }
            InstalledState::InstalledUnknown => {
                ui.label("✔ Rando installed");
            }
            InstalledState::Installed(installed) => {
                ui.label(format!("✔ Rando installed ({installed})"));
                self.draw_update_line(ui, installed);
            }
        });
    }

    fn draw_update_line(&mut self, ui: &mut Ui, installed: RandoVersion) {
        match self.newest_version_available {
            NewestState::Unknown => {}
            NewestState::Checking => {
                Aligner::center_top()
                    .layout(Layout::right_to_left(Align::Center))
                    .show(ui, |ui| {
                        let resp = ui.label("Checking for updates...");
                        Spinner::new().size(resp.rect.height()).ui(ui);
                    });
            }
            NewestState::Error => {
                ui.colored_label(Color32::RED, "✖ Error checking for updates");
            }
            NewestState::Version(newest) => {
                if installed == newest {
                    ui.colored_label(Color32::GREEN, "✔ Already on newest version");
                } else {
                    self.draw_install_button(ui, &format!("Update to v{newest}"), false);
                }
            }
        }
    }

    fn draw_install_button(&mut self, ui: &mut Ui, text: &str, big: bool) {
        ui.scope(|ui| {
            ui.style_mut().text_styles.insert(
                TextStyle::Button,
                FontId::new(if big { 20. } else { 13. }, FontFamily::Proportional),
            );

            let color = self.theme_color(Color32::LIGHT_BLUE, Color32::from_rgb(77, 140, 156));

            let style = ui.style_mut();
            let widgets = &mut style.visuals.widgets;
            widgets.inactive.weak_bg_fill = widgets.inactive.weak_bg_fill.lerp_to_gamma(color, 0.5);
            widgets.hovered.weak_bg_fill = widgets.hovered.weak_bg_fill.lerp_to_gamma(color, 0.5);
            widgets.active.weak_bg_fill = widgets.active.weak_bg_fill.lerp_to_gamma(color, 0.5);

            if ui.button(text).clicked() {
                self.download_update();
            }
        });
    }
}
impl Inner {
    #[instrument(skip(self))]
    fn download_update(&mut self) {
        if let Some(modal_message) = &self.modal_message {
            warn!(
                ?modal_message,
                "Some modal action is already in progress, doing nothing"
            );
            return;
        }

        self.modal_message = Some("Installing Randomizer...".to_owned());

        let game_dir = self.settings.game_dir.clone();
        let all_dlls = self.all_dlls.clone();

        info!("Downloading update");
        self.run_off_thread(
            move || -> color_eyre::Result<()> {
                let dll = download_dll()?;
                install_new_dll(&game_dir, &dll, &all_dlls)?;
                Ok(())
            },
            |app, result| {
                if let Err(err) = result {
                    error!(?err, "Error downloading update");
                    app.error_message = Some("Failed to ".into());
                }

                app.modal_message = None;
                app.update_dlls();
            },
        );
    }
}
