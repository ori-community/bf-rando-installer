use crate::dll_management::{OriDll, OriDllKind, install_dll};
use crate::gui::{Inner, InstalledState, NewestState, open_file_button};
use crate::orirando_website::Endpoint;
use crate::rando_files::backup_previous_rando_file;
use eframe::egui::{ComboBox, Memory, Ui};
use std::cmp::max;
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
            ComboBox::from_label("Switch version")
                .selected_text(format_selected_dll(
                    self.settings.stay_on_latest,
                    self.current_dll.as_ref(),
                ))
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.settings.stay_on_latest, self.render_latest())
                        .clicked()
                    {
                        ui.memory_mut(Memory::close_popup);
                        if !self.settings.stay_on_latest {
                            self.switch_to_latest();
                        }
                    }

                    let mut new_version = None;
                    for dll in self.all_dlls.iter().filter(|&dll| self.dll_visible(dll)) {
                        let label = format_dll(dll);
                        let selected = if let Some(cur) = &self.current_dll {
                            !self.settings.stay_on_latest && cur.kind == dll.kind
                        } else {
                            false
                        };

                        if ui.selectable_label(selected, label).clicked() {
                            new_version = Some(dll);
                            ui.memory_mut(Memory::close_popup);
                        }
                    }

                    if let Some(new_version) = new_version
                        && (different_version(new_version, self.current_dll.as_ref())
                            || self.settings.stay_on_latest)
                    {
                        self.switch_to_version(new_version.clone(), false);
                    }
                });
        });
    }

    fn dll_visible(&self, dll: &OriDll) -> bool {
        let OriDllKind::Rando(v) = dll.kind else {
            return true;
        };

        !v.is_beta() || self.settings.show_beta
    }

    #[instrument(skip_all)]
    fn draw_open_directories(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            open_file_button(ui, "Open game/seed folder", || {
                self.settings.game_dir.install.clone()
            });

            if *self
                .has_old_seeds
                .get_cached(self.settings.game_dir.install.clone())
            {
                open_file_button(ui, "Show old seeds", || {
                    self.settings.game_dir.install.join("seeds")
                });
            }
        });

        ui.add_enabled_ui(*self.has_current_seed.get_cached(self.settings.game_dir.install.clone()), |ui| {
            if ui.button("Archive current seed").on_hover_text("Archives the current seed into the old seeds directory, including the stats.txt file if it exists. Happens automatically when playing a new seed.").clicked() {
                if let Err(err) = backup_previous_rando_file(&self.settings.game_dir) {
                    error!(?err, "Failed to archive current seed");
                    self.push_error("Failed to archive seed");
                }

                self.has_current_seed
                    .update(self.settings.game_dir.install.clone());
            }
        });
    }

    fn render_latest(&self) -> String {
        let latest_installed =
            if let InstalledState::Installed(v, ..) = self.newest_version_installed {
                Some(v)
            } else {
                None
            };

        let latest_available =
            if let NewestState::Version(v, _endpoint) = self.newest_version_available {
                Some(v)
            } else {
                None
            };

        let latest = max(latest_installed, latest_available);

        if let Some(latest) = latest {
            format!("Latest (v{latest})")
        } else {
            "Latest".into()
        }
    }
}

impl Inner {
    #[instrument(skip(self, version))]
    fn switch_to_version(&mut self, version: OriDll, stay_on_latest: bool) {
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
                    false
                } else {
                    true
                }
            },
            move |app, success| {
                app.modal_message = None;
                app.update_dlls();
                if success {
                    app.settings.stay_on_latest = stay_on_latest;
                } else {
                    app.push_error("Failed to switch version");
                }
            },
        );
    }

    #[instrument(skip(self))]
    fn switch_to_latest(&mut self) {
        let latest_installed =
            if let InstalledState::Installed(v, dll) = self.newest_version_installed.clone() {
                Some((v, dll))
            } else {
                None
            };

        let (latest_available, endpoint) =
            if let NewestState::Version(v, endpoint) = self.newest_version_available {
                (Some(v), Some(endpoint))
            } else {
                (None, None)
            };

        match (latest_installed, latest_available) {
            (Some((installed, dll)), Some(available)) if installed >= available => {
                self.switch_to_version(dll, true);
            }
            (Some((_v, dll)), None) => {
                self.switch_to_version(dll, true);
            }
            _ => {
                self.settings.stay_on_latest = true;
                self.download_update(endpoint.unwrap_or(Endpoint::Stable));
            }
        }
    }
}

fn format_selected_dll(stay_on_latest: bool, dll: Option<&OriDll>) -> String {
    match (stay_on_latest, dll) {
        (false, None) => "<None>".into(),
        (false, Some(dll)) => format_dll(dll),
        (true, None) => "Latest".into(),
        (true, Some(dll)) => match dll.kind {
            OriDllKind::Rando(v) => format!("Latest (v{v})"),
            _ => "Latest".into(),
        },
    }
}

fn format_dll(dll: &OriDll) -> String {
    match dll.kind {
        OriDllKind::Vanilla => "Vanilla".to_owned(),
        OriDllKind::Rando(v) => format!("Rando v{v}"),
        OriDllKind::UnknownRando(_) => format!("Rando [{}]", dll.display_name),
    }
}

fn different_version(new: &OriDll, old: Option<&OriDll>) -> bool {
    old.is_none_or(|old| old.kind != new.kind)
}
