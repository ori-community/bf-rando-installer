use crate::dll_management::{OriDll, OriDllKind, install_dll};
use crate::gui::{Inner, open_file_button};
use eframe::egui::{ComboBox, Ui};
use tracing::{error, info, instrument, warn};

impl Inner {
    #[instrument(skip_all)]
    pub(super) fn draw_rando_ui(&mut self, ui: &mut Ui) {
        ui.separator();
        self.draw_version_selector(ui);
        ui.separator();
        self.draw_open_directories(ui);
    }

    #[instrument(skip_all)]
    fn draw_version_selector(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Switch version");

            ComboBox::from_id_salt("Select version CB")
                .selected_text(format_dll(&self.current_dll))
                .show_ui(ui, |ui| {
                    let mut new_version = self.current_dll.clone();
                    for dll in self.all_dlls.iter().cloned().map(Some) {
                        let label = format_dll(&dll);
                        ui.selectable_value(&mut new_version, dll, label);
                    }

                    if different_version(&new_version, &self.current_dll) {
                        if let Some(version) = new_version {
                            self.switch_to_version(version);
                        } else {
                            error!("Selected <none> version. This shouldn't be possible (doing nothing)");
                        }
                    }
                });
        });
    }

    #[instrument(skip_all)]
    fn draw_open_directories(&self, ui: &mut Ui) {
        open_file_button(ui, "Open seed folder", || {
            self.settings.game_dir.install.clone()
        });
    }
}

impl Inner {
    #[instrument(skip(self, version))]
    fn switch_to_version(&mut self, version: OriDll) {
        if let Some(modal_message) = &self.modal_message {
            warn!(
                ?modal_message,
                "Some modal action is already in progress, doing nothing"
            );
            return;
        }

        info!(to_install=?version, "Switching version");
        self.modal_message = Some("Switching version...".to_owned());

        let game_dir = self.settings.game_dir.clone();
        let all_dlls = self.all_dlls.clone();

        self.run_off_thread(
            move || {
                if let Err(err) = install_dll(&game_dir, &version, &all_dlls) {
                    error!(?version, ?err, "Couldn't install new dll");
                    true
                } else {
                    false
                }
            },
            |app, errored| {
                app.modal_message = None;
                app.update_dlls();
                if errored {
                    app.error_message = Some("Failed to switch version".into());
                }
            },
        );
    }
}

fn format_dll(dll: &Option<OriDll>) -> String {
    match dll {
        None => "<None>".to_owned(),
        Some(dll) => match dll.kind {
            OriDllKind::Vanilla => "Vanilla".to_owned(),
            OriDllKind::Rando(v) => format!("Rando v{v}"),
            OriDllKind::UnknownRando(_) => format!("Rando [{}]", dll.display_name),
        },
    }
}

fn different_version(new: &Option<OriDll>, old: &Option<OriDll>) -> bool {
    match (new, old) {
        (Some(a), Some(b)) => a.kind != b.kind,
        (Some(_), None) => true,
        _ => false,
    }
}
