use crate::LOGFILE;
use crate::dll_classifier::RandoVersion;
use crate::dll_management::{OriDll, OriDllKind, install_dll, install_new_dll, search_game_dir};
use crate::orirando::{check_version, download_dll};
use crate::settings::{GameDir, Settings, search_for_game_dir, verify_game_dir};
use color_eyre::Result;
use color_eyre::eyre::eyre;
use eframe::NativeOptions;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, ComboBox, Context, FontId, Frame, IconData, Id,
    InnerResponse, Layout, Margin, Modal, Sides, Spinner, TextStyle, Theme, ThemePreference, Ui,
    UiBuilder, ViewportBuilder, ViewportCommand, Widget,
};
use eframe::epaint::FontFamily;
use egui_alignments::Aligner;
use image::{ImageFormat, load_from_memory_with_format};
use opener::reveal;
use rfd::FileDialog;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::{env, mem};
use tracing::{Metadata, Span, debug, error, info, info_span, instrument, warn};

#[instrument(skip(settings))]
pub fn run_gui(settings: Settings) -> Result<()> {
    let icon = load_from_memory_with_format(include_bytes!("../icon.ico"), ImageFormat::Ico)
        .expect("invalid icon file");
    let icon = IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_rgba8().into_vec(),
    };

    let options = NativeOptions {
        centered: true,
        viewport: ViewportBuilder::default()
            .with_inner_size([300., 250.])
            .with_icon(icon),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Ori Rando Installer",
        options,
        Box::new(|cc| {
            adjust_themes(&cc.egui_ctx);
            cc.egui_ctx.set_theme(settings.theme_preference);
            Ok(Box::new(App::new(settings, cc.egui_ctx.clone())))
        }),
    );

    result.map_err(|e| eyre!("Error running gui: {e:?}"))
}

struct App {
    inner: Arc<Mutex<Inner>>,
}

impl App {
    fn new(settings: Settings, egui_ctx: Context) -> App {
        let app = Self {
            inner: Arc::new(Mutex::new(Inner::new(settings))),
        };

        let mut inner = app.inner.lock().unwrap();
        inner.weak_self = Arc::downgrade(&app.inner);
        inner.egui_ctx = egui_ctx;
        inner.update_dlls();
        inner.check_newest();
        drop(inner);

        app
    }
}

#[derive(Default, Eq, PartialEq)]
enum InstalledState {
    #[default]
    Unknown,
    Checking,
    None,
    InstalledUnknown,
    Installed(RandoVersion),
}

#[derive(Default, Debug, Eq, PartialEq)]
enum NewestState {
    #[default]
    Unknown,
    Checking,
    Error,
    Version(RandoVersion),
}

#[derive(Default)]
struct Inner {
    weak_self: Weak<Mutex<Inner>>,
    egui_ctx: Context,
    show_settings: bool,
    settings: Settings,
    current_dll: Option<OriDll>,
    all_dlls: Vec<OriDll>,
    newest_version_installed: InstalledState,
    newest_version_available: NewestState,
    modal_message: Option<String>,
    error_message: Option<String>,
    modal_uis: Vec<(AppModal, Box<DynModalUi>)>,
}

type DynModalUi = dyn FnMut(&mut Inner, &mut Ui, &mut AppModal) + Send;

struct AppModal {
    dismissable: bool,
    open: bool,
}

impl Default for AppModal {
    fn default() -> Self {
        Self {
            dismissable: false,
            open: true,
        }
    }
}

impl AppModal {
    fn new() -> Self {
        Default::default()
    }
}

impl AppModal {
    fn dismissable(mut self, dismissable: bool) -> Self {
        self.dismissable = dismissable;
        self
    }
}

impl AppModal {
    fn close(&mut self) {
        self.open = false;
    }
}

impl Inner {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }
}

impl eframe::App for App {
    #[instrument(skip(self, ctx, _frame))]
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut app = self.inner.lock().unwrap();

        CentralPanel::default().show(ctx, |ui| {
            top_right(ui, |ui| {
                ui.toggle_value(&mut app.show_settings, "⛭").on_hover_text("Settings");
            });

            ui.vertical_centered(|ui| {
                ui.heading("Ori Rando Installer");
            });

            if !app.settings.game_dir.is_set() {
                ui.label("Installation of Ori and the Blind Forest: Definitive Edition not found.");
                ui.label("Note: The randomizer is only compatible with the Definitive Edition, not the original.");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Please select the installation directory:");
                    let dir_changed = app.draw_choose_game_dir_button(ui);
                    if dir_changed {
                        app.settings.save_async();
                    }
                });
            } else if app.show_settings {
                app.draw_settings_ui(ui);
            } else {
                app.draw_rando_version(ui);
            }

            app.draw_bottom_row(ui);

            if let Some((mut modal, mut modal_ui)) = app.modal_uis.pop() {
                let resp = Modal::new(Id::new("ui_modal")).show(ctx, |ui| {
                    modal_ui(&mut app, ui, &mut modal);
                });

                if modal.dismissable && resp.should_close() {
                    modal.close();
                }

                if modal.open {
                    app.modal_uis.push((modal, modal_ui));
                }
            }

            if let Some(msg) = &app.modal_message {
                Modal::new(Id::new("modal message")).show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(msg);
                        ui.spinner();
                    });
                });
            }

            app.draw_error_modal(ui);
        });
    }
}

impl Inner {
    fn show_modal_ui(
        &mut self,
        modal: AppModal,
        add_contents: impl FnMut(&mut Self, &mut Ui, &mut AppModal) + Send + 'static,
    ) {
        self.modal_uis.push((modal, Box::new(add_contents)));
    }

    #[instrument(skip(self, ui))]
    fn draw_settings_ui(&mut self, ui: &mut Ui) {
        let old_settings = self.settings.clone();

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Theme");
                self.settings.theme_preference.radio_buttons(ui);
            });

            self.draw_game_dir_setting(ui);

            Self::draw_show_log_button(ui);
        });

        if self.settings != old_settings {
            self.settings.save_async();
            ui.ctx()
                .options_mut(|o| o.theme_preference = self.settings.theme_preference);
        }
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
                self.update_dlls();
            }
        });
    }

    fn draw_choose_game_dir_button(&mut self, ui: &mut Ui) -> bool {
        if ui.button("Choose...").clicked() {
            let dir = FileDialog::new().pick_folder();
            if let Some(dir) = dir {
                let game_dir = GameDir::new(dir);
                if verify_game_dir(&game_dir) {
                    self.settings.game_dir = game_dir;
                    self.update_dlls();
                    return true;
                } else {
                    self.show_invalid_game_dir_modal();
                }
            }
        }
        false
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

    fn draw_rando_version(&mut self, ui: &mut Ui) {
        match self.newest_version_installed {
            InstalledState::Unknown => {}
            InstalledState::Checking => {
                ui.vertical_centered(|ui| {
                    ui.label("Loading installed versions...");
                });
            }
            InstalledState::None => {
                ui.vertical_centered(|ui| {
                    self.draw_install_button(ui, "Install Randomizer", true);
                });
            }
            InstalledState::InstalledUnknown => {
                ui.vertical_centered(|ui| {
                    ui.label("✔ Rando installed");
                });
                self.draw_version_selector(ui);
            }
            InstalledState::Installed(installed) => {
                ui.vertical_centered(|ui| {
                    ui.label(format!("✔ Rando installed ({installed})"));
                    self.draw_update_line(ui, installed);
                });
                self.draw_version_selector(ui);
                self.draw_open_directories(ui);
                self.draw_open_files(ui);
            }
        }
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

    #[instrument(skip(self, ui))]
    fn draw_version_selector(&mut self, ui: &mut Ui) {
        ui.separator();
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
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("Open game directory (where you place the randomizer.dat):");
            open_file_button(ui, "Open", || self.settings.game_dir.install.clone());
        });
    }

    #[instrument(skip_all)]
    fn draw_open_files(&self, ui: &mut Ui) {
        ui.separator();
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

    #[instrument(skip(self, ui))]
    fn draw_error_modal(&mut self, ui: &mut Ui) {
        if let Some(msg) = &self.error_message {
            let padding = ui.style().spacing.interact_size.y as _;

            let frame = Frame::popup(ui.style())
                .fill(self.theme_color(
                    Color32::from_rgb(255, 102, 102),
                    Color32::from_rgb(122, 0, 0),
                ))
                .inner_margin(Margin {
                    left: padding,
                    right: padding,
                    top: padding,
                    bottom: padding / 2,
                })
                .stroke((0., Color32::default()));

            let modal =
                Modal::new(Id::new("error modal"))
                    .frame(frame)
                    .show(&self.egui_ctx, |ui| {
                        ui.heading("Error");
                        ui.label(msg);
                        ui.label("");
                        Sides::new()
                            .show(
                                ui,
                                |ui| {
                                    Self::draw_show_log_button(ui);
                                },
                                |ui| ui.button("Ok").clicked(),
                            )
                            .1
                    });

            if modal.inner || modal.should_close() {
                self.error_message = None;
            }
        }
    }

    fn draw_show_log_button(ui: &mut Ui) {
        if let Some(path) = LOGFILE.get() {
            if ui.button("Show logs").clicked() {
                let result = reveal(path);
                if let Err(err) = result {
                    error!(?err, "Couldn't show log file");
                }
            }
        }
    }

    fn draw_bottom_row(&mut self, ui: &mut Ui) {
        bottom_left(ui, |ui| {
            #[allow(clippy::collapsible_else_if)]
            if ui.ctx().theme() == Theme::Dark {
                if ui
                    .add(Button::new("☀").frame(false))
                    .on_hover_text("Switch to light mode")
                    .clicked()
                {
                    ui.ctx().set_theme(Theme::Light);
                    self.settings.theme_preference = ThemePreference::Light;
                    self.settings.save_async();
                }
            } else {
                if ui
                    .add(Button::new("🌙").frame(false))
                    .on_hover_text("Switch to dark mode")
                    .clicked()
                {
                    ui.ctx().set_theme(Theme::Dark);
                    self.settings.theme_preference = ThemePreference::Dark;
                    self.settings.save_async();
                }
            }
        });

        bottom_right(ui, |ui| {
            if ui.button("Close").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
        });
    }
}

impl Inner {
    fn theme_color(&self, light: Color32, dark: Color32) -> Color32 {
        if self.egui_ctx.theme() == Theme::Light {
            light
        } else {
            dark
        }
    }

    fn run_off_thread<C, S, R>(&self, calc: C, sync: S)
    where
        C: (FnOnce() -> R) + Send + 'static,
        S: (FnOnce(&mut Self, R)) + Send + 'static,
    {
        let weak_self = self.weak_self.clone();

        let current_span = Span::current();

        thread::spawn(move || {
            let span =
                info_span!("run_off_thread", source=?current_span.metadata().map(Metadata::name));
            span.follows_from(current_span);

            let value = info_span!(parent: &span, "calc_func").in_scope(calc);

            if let Some(app) = weak_self.upgrade() {
                let mut app = app.lock().unwrap();
                info_span!(parent: &span, "sync_func").in_scope(|| sync(&mut app, value));
                app.egui_ctx.request_repaint();
            } else {
                info!("App destroyed, not running sync func");
            }
        });
    }

    #[instrument(skip(self))]
    fn update_dlls(&mut self) {
        if !self.settings.game_dir.is_set() {
            debug!("Tried to update dlls, but no game dir is set. Aborting.");
            return;
        }

        if mem::replace(&mut self.newest_version_installed, InstalledState::Checking)
            == InstalledState::Checking
        {
            warn!("Tried to update dlls, while an update is already in progress. Aborting.");
            return;
        }

        info!("Updating dlls...");

        let game_dir = self.settings.game_dir.clone();
        self.run_off_thread(
            move || {
                let (current, all) = match search_game_dir(&game_dir) {
                    Ok(v) => v,
                    Err(e) => {
                        error!(?e, "Couldn't update dlls");
                        return None;
                    }
                };

                let newest = {
                    let newest_known = all
                        .iter()
                        .filter_map(|dll| match dll.kind {
                            OriDllKind::Rando(v) => Some(v),
                            _ => None,
                        })
                        .max();

                    let has_unknown = all
                        .iter()
                        .any(|dll| matches!(dll.kind, OriDllKind::UnknownRando(_)));

                    match (newest_known, has_unknown) {
                        (Some(v), _) => InstalledState::Installed(v),
                        (None, true) => InstalledState::InstalledUnknown,
                        _ => InstalledState::None,
                    }
                };

                Some((current, all, newest))
            },
            |app, dlls| {
                let Some((current, all, newest)) = dlls else {
                    app.newest_version_installed = InstalledState::None;
                    app.error_message = Some("Failed to load installed versions".into());
                    return;
                };

                info!("Updated dlls");
                app.current_dll = current;
                app.all_dlls = all;
                app.newest_version_installed = newest;
            },
        );
    }

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

    #[instrument(skip(self))]
    fn check_newest(&mut self) {
        self.newest_version_available = NewestState::Checking;

        info!("Checking for newest dll available");
        self.run_off_thread(
            || match check_version() {
                Ok(v) => NewestState::Version(v),
                Err(err) => {
                    error!(?err, "Failed to check newest available version");
                    NewestState::Error
                }
            },
            |app, newest| {
                info!(?newest, "Retrieved newest version available");
                app.newest_version_available = newest;
            },
        );
    }

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
            move || -> Result<()> {
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

    fn rando_install_path(&self, file: &str) -> PathBuf {
        self.settings.game_dir.install.join(file)
    }
}

fn adjust_themes(ctx: &Context) {
    ctx.style_mut_of(Theme::Light, |style| {
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(30);
        style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(30);
    });

    ctx.style_mut_of(Theme::Dark, |style| {
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(235);
        style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(235);
    });
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

fn game_app_path(file: &str) -> PathBuf {
    let Some(local_appdata) = env::var_os("LOCALAPPDATA") else {
        return PathBuf::new();
    };
    let mut path = PathBuf::from(local_appdata);
    path.extend(["Ori and the Blind Forest DE", file]);
    path
}

fn open_file_button(ui: &mut Ui, button_text: &str, get_path: impl Fn() -> PathBuf) {
    if ui
        .button(button_text)
        .on_hover_ui(|ui| {
            ui.label(get_path().to_string_lossy());
        })
        .clicked()
    {
        open_file(&get_path());
    }
}

#[instrument]
fn open_file(path: &Path) {
    if let Err(err) = opener::open(path) {
        error!(?err, "Could not open file");
    }
}

/// Like `ui.scope(add_contents)` but forgets the size of the contents.
/// So any widgets added to `ui` after this call will behave exactly the same way as if `forgetful_scope` wasn't called.
/// Be careful: This makes it easy to have multiple widgets overlap each other.
fn forgetful_scope<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    forgetful_scope_dyn(ui, Box::new(add_contents))
}

fn forgetful_scope_dyn<'c, R>(
    ui: &mut Ui,
    add_contents: Box<dyn FnOnce(&mut Ui) -> R + 'c>,
) -> InnerResponse<R> {
    let mut child_ui = ui.new_child(UiBuilder::new());
    let ret = add_contents(&mut child_ui);
    let response = child_ui.response();
    InnerResponse::new(ret, response)
}

fn top_right<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    forgetful_scope(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Min), add_contents)
            .inner
    })
}

fn bottom_left<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    forgetful_scope(ui, |ui| {
        ui.with_layout(Layout::left_to_right(Align::Max), add_contents)
    })
    .inner
}

fn bottom_right<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    forgetful_scope(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Max), add_contents)
            .inner
    })
}
