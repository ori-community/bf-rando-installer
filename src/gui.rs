use color_eyre::eyre::eyre;
use eframe::NativeOptions;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, ComboBox, Context, FontId, Frame, Id, Layout, Margin,
    Modal, Sides, Spinner, TextStyle, Theme, Ui, ViewportBuilder, ViewportCommand, Widget,
};
use std::mem::replace;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use tracing::{Metadata, Span, error, info, info_span, instrument, warn};

use crate::LOGFILE;
use crate::dll_classifier::RandoVersion;
use crate::dll_management::{
    GameDir, OriDll, OriDllKind, install_dll, install_new_dll, search_game_dir,
};
use crate::orirando::{check_version, download_dll};
use color_eyre::Result;
use eframe::epaint::FontFamily;
use egui_alignments::Aligner;
use opener::open;

#[instrument]
pub fn run_gui(ori_path: PathBuf) -> Result<()> {
    let options = NativeOptions {
        centered: true,
        viewport: ViewportBuilder::default().with_inner_size([300., 200.]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Ori Rando Installer",
        options,
        Box::new(|cc| {
            adjust_themes(&cc.egui_ctx);
            Ok(Box::new(App::new(ori_path, cc.egui_ctx.clone())))
        }),
    );

    result.map_err(|e| eyre!("Error running gui: {e:?}"))
}

struct App {
    inner: Arc<Mutex<Inner>>,
}

impl App {
    fn new(ori_path: PathBuf, egui_ctx: Context) -> App {
        let app = Self {
            inner: Arc::new(Mutex::new(Inner::new(GameDir::new(ori_path)))),
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
    _ori_path: PathBuf,
    game_dir: GameDir,
    current_dll: Option<OriDll>,
    all_dlls: Vec<OriDll>,
    newest_version_installed: InstalledState,
    newest_version_available: NewestState,
    modal_message: Option<String>,
    error_message: Option<String>,
}

impl Inner {
    fn new(game_dir: GameDir) -> Self {
        Self {
            game_dir,
            ..Self::default()
        }
    }
}

impl eframe::App for App {
    #[instrument(skip(self, ctx, _frame))]
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut app = self.inner.lock().unwrap();

        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Ori Rando Installer");

                Self::draw_rando_version(&mut app, ui);
            });

            Self::draw_bottom_row(ctx, ui);

            if let Some(msg) = &app.modal_message {
                Modal::new(Id::new("modal message")).show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(msg);
                        ui.spinner();
                    });
                });
            }

            Self::draw_error_modal(&mut app, ui);
        });
    }
}

impl App {
    fn draw_rando_version(app: &mut Inner, ui: &mut Ui) {
        match app.newest_version_installed {
            InstalledState::Unknown => {}
            InstalledState::Checking => {
                ui.label("Loading installed versions...");
            }
            InstalledState::None => {
                Self::draw_install_button(app, ui, "Install Randomizer", true);
            }
            InstalledState::InstalledUnknown => {
                ui.label("✔ Rando installed");
                Self::draw_version_selector(app, ui);
            }
            InstalledState::Installed(installed) => {
                ui.label(format!("✔ Rando installed ({installed})"));
                Self::draw_update_line(app, ui, installed);
                Self::draw_version_selector(app, ui);
            }
        }
    }

    fn draw_update_line(app: &mut Inner, ui: &mut Ui, installed: RandoVersion) {
        match app.newest_version_available {
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
                    Self::draw_install_button(app, ui, &format!("Update to v{newest}"), false);
                }
            }
        }
    }

    fn draw_install_button(app: &mut Inner, ui: &mut Ui, text: &str, big: bool) {
        ui.scope(|ui| {
            ui.style_mut().text_styles.insert(
                TextStyle::Button,
                FontId::new(if big { 20. } else { 13. }, FontFamily::Proportional),
            );

            let color = app.theme_color(Color32::LIGHT_BLUE, Color32::from_rgb(77, 140, 156));

            let style = ui.style_mut();
            let widgets = &mut style.visuals.widgets;
            widgets.inactive.weak_bg_fill = widgets.inactive.weak_bg_fill.lerp_to_gamma(color, 0.5);
            widgets.hovered.weak_bg_fill = widgets.hovered.weak_bg_fill.lerp_to_gamma(color, 0.5);
            widgets.active.weak_bg_fill = widgets.active.weak_bg_fill.lerp_to_gamma(color, 0.5);

            if ui.button(text).clicked() {
                app.download_update();
            }
        });
    }

    fn draw_version_selector(app: &mut Inner, ui: &mut Ui) {
        ui.label("");
        ui.horizontal(|ui| {
            ui.label("Switch version");

            ComboBox::from_id_salt("Select version CB")
                .selected_text(format_dll(&app.current_dll))
                .show_ui(ui, |ui| {
                    let mut new_version = app.current_dll.clone();
                    for dll in app.all_dlls.iter().cloned().map(Some) {
                        let label = format_dll(&dll);
                        ui.selectable_value(&mut new_version, dll, label);
                    }

                    if different_version(&new_version, &app.current_dll) {
                        if let Some(version) = new_version {
                            app.switch_to_version(version);
                        } else {
                            error!("Selected <none> version. This shouldn't be possible (doing nothing)");
                        }
                    }
                });
        });
    }

    fn draw_bottom_row(ctx: &Context, ui: &mut Ui) {
        ui.with_layout(Layout::left_to_right(Align::Max), |ui| {
            #[allow(clippy::collapsible_else_if)]
            if ctx.theme() == Theme::Dark {
                if ui
                    .add(Button::new("☀").frame(false))
                    .on_hover_text("Switch to light mode")
                    .clicked()
                {
                    ctx.set_theme(Theme::Light);
                }
            } else {
                if ui
                    .add(Button::new("🌙").frame(false))
                    .on_hover_text("Switch to dark mode")
                    .clicked()
                {
                    ctx.set_theme(Theme::Dark);
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });
    }

    fn draw_error_modal(app: &mut Inner, ui: &mut Ui) {
        if let Some(msg) = &app.error_message {
            let padding = ui.style().spacing.interact_size.y as _;

            let frame = Frame::popup(ui.style())
                .fill(app.theme_color(
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

            let modal = Modal::new(Id::new("error modal"))
                .frame(frame)
                .show(&app.egui_ctx, |ui| {
                    ui.heading("Error");
                    ui.label(msg);
                    ui.label("");
                    Sides::new()
                        .show(
                            ui,
                            |ui| {
                                Self::draw_open_log_button(ui);
                            },
                            |ui| ui.button("Ok").clicked(),
                        )
                        .1
                });

            if modal.inner || modal.should_close() {
                app.error_message = None;
            }
        }
    }

    fn draw_open_log_button(ui: &mut Ui) {
        if let Some(path) = LOGFILE.get() {
            if ui.button("Open log file").clicked() {
                let result = open(path);
                if let Err(err) = result {
                    error!(?err, "Couldn't open log file");
                }
            }
        }
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
        if replace(&mut self.newest_version_installed, InstalledState::Checking)
            == InstalledState::Checking
        {
            warn!("Tried to update dlls, while an update is already in progress. Aborting.");
            return;
        }

        info!("Updating dlls...");

        let game_dir = self.game_dir.clone();
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

        let game_dir = self.game_dir.clone();
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

        let game_dir = self.game_dir.clone();
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
