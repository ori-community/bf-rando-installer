use crate::dll_classifier::RandoVersion;
use crate::dll_management::OriDllKind;
use crate::gui::{Inner, InstalledState, NewestState};
use eframe::egui::{Align, Color32, FontFamily, FontId, Layout, Spinner, TextStyle, Ui, Widget};
use egui_alignments::Aligner;

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
            InstalledState::Installed(newest_installed, ..) => {
                let current_is_newest = self
                    .current_dll
                    .as_ref()
                    .is_some_and(|d| d.kind == OriDllKind::Rando(newest_installed));

                if current_is_newest {
                    ui.label(format!("✔ Rando installed ({newest_installed})"));
                } else {
                    ui.label(format!("✔ Rando installed (up to {newest_installed})"));
                }
                self.draw_update_line(ui, newest_installed, current_is_newest);
            }
        });
    }

    fn draw_update_line(&mut self, ui: &mut Ui, installed: RandoVersion, current_is_newest: bool) {
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
                if installed >= newest {
                    if current_is_newest {
                        let text = if self.just_updated {
                            "✔ Updated to newest version"
                        } else {
                            "✔ Already on newest version"
                        };
                        ui.colored_label(Color32::GREEN, text);
                    } else {
                        ui.label("No new version available");
                    }
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
