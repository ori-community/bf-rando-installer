use crate::LOGFILE;
use crate::dll_classifier::RandoVersion;
use crate::dll_management::{OriDll, OriDllKind, install_new_dll, search_game_dir};
use crate::orirando::{check_version, download_dll};
use crate::rando_files::play_rando_file;
use crate::settings::Settings;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use eframe::NativeOptions;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, Context, Frame, Galley, IconData, Id, InnerResponse,
    LayerId, Layout, Margin, Modal, Order, Sides, TextStyle, Theme, ThemePreference, Ui, UiBuilder,
    ViewportBuilder, ViewportCommand,
};
use image::{ImageFormat, load_from_memory_with_format};
use opener::reveal;
use std::ffi::OsStr;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use tracing::{Metadata, Span, debug, error, info, info_span, instrument, warn};

mod app_settings;
mod game_settings;
mod rando;
mod version_row;

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
        "Ori DE Randomizer",
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

#[derive(Default)]
struct Inner {
    weak_self: Weak<Mutex<Inner>>,
    egui_ctx: Context,
    show_settings: bool,
    settings: Settings,
    prev_settings: Settings,
    active_screen: ActiveScreen,
    current_dll: Option<OriDll>,
    all_dlls: Vec<OriDll>,
    newest_version_installed: InstalledState,
    newest_version_available: NewestState,
    modal_message: Option<String>,
    error_message: Option<String>,
    modal_uis: Vec<(AppModal, Box<DynModalUi>)>,
}

#[derive(Default, Clone, Eq, PartialEq)]
enum InstalledState {
    #[default]
    Unknown,
    Checking,
    None,
    InstalledUnknown,
    Installed(RandoVersion, OriDll),
}

#[derive(Debug, Default, Eq, PartialEq)]
enum NewestState {
    #[default]
    Unknown,
    Checking,
    Error,
    Version(RandoVersion),
}

#[derive(Default, Eq, PartialEq)]
enum ActiveScreen {
    #[default]
    Rando,
    GameSettings,
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
        Self::default()
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
            settings: settings.clone(),
            prev_settings: settings,
            ..Self::default()
        }
    }
}

impl eframe::App for App {
    #[instrument(skip(self, ctx, _frame))]
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut app = self.inner.lock().unwrap();
        app.render(ctx);
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
    fn draw_error_modal(&mut self, ui: &mut Ui) {
        if let Some(msg) = &self.error_message {
            #[allow(clippy::cast_possible_truncation)]
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
}

impl Inner {
    fn render(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            top_right(ui, |ui| {
                ui.toggle_value(&mut self.show_settings, "⛭").on_hover_text("Settings");
            });

            ui.vertical_centered(|ui| {
                ui.heading("Ori DE Randomizer");
            });

            if !self.settings.game_dir.is_set() {
                ui.label("Installation of Ori and the Blind Forest: Definitive Edition not found.");
                ui.label("Note: The randomizer is only compatible with the Definitive Edition, not the original.");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Please select the installation directory:");
                    self.draw_choose_game_dir_button(ui);
                });
            } else {
                self.dnd_seed(ctx);

                if self.show_settings {
                    self.draw_settings_ui(ui);
                } else {
                    self.draw_rando_version(ui);
                    if matches!(self.newest_version_installed, InstalledState::InstalledUnknown | InstalledState::Installed(..)) {
                        self.draw_main_ui(ui);
                    }
                }
            }

            self.draw_bottom_row(ui);

            if let Some((mut modal, mut modal_ui)) = self.modal_uis.pop() {
                let resp = Modal::new(Id::new("ui_modal")).show(ctx, |ui| {
                    modal_ui(self, ui, &mut modal);
                });

                if modal.dismissable && resp.should_close() {
                    modal.close();
                }

                if modal.open {
                    self.modal_uis.push((modal, modal_ui));
                }
            }

            if let Some(msg) = &self.modal_message {
                Modal::new(Id::new("modal message")).show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(msg);
                        ui.spinner();
                    });
                });
            }

            self.draw_error_modal(ui);
        });

        if self.settings != self.prev_settings {
            if self.settings.game_dir != self.prev_settings.game_dir {
                self.update_dlls();
            }

            self.prev_settings = self.settings.clone();
            self.settings.save_async();
            ctx.options_mut(|o| o.theme_preference = self.settings.theme_preference);
        }
    }

    fn draw_main_ui(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.active_screen, ActiveScreen::Rando, "Rando");
            ui.selectable_value(
                &mut self.active_screen,
                ActiveScreen::GameSettings,
                "Game Settings",
            );
        });

        match self.active_screen {
            ActiveScreen::Rando => {
                self.draw_rando_ui(ui);
            }
            ActiveScreen::GameSettings => {
                self.draw_game_settings_ui(ui);
            }
        }
    }

    #[instrument(skip_all)]
    fn dnd_seed(&mut self, ctx: &Context) {
        Self::dnd_seed_hover(ctx);
        self.dnd_seed_dropped(ctx);
    }

    fn dnd_seed_hover(ctx: &Context) {
        let Some(Some(file_path)) = ctx.input(|i| {
            let hovered = &i.raw.hovered_files;
            (hovered.len() == 1).then(|| hovered[0].path.clone())
        }) else {
            return;
        };

        if !Self::valid_seed_file(&file_path) {
            return;
        }

        let Some(file_name) = file_path.file_name().map(OsStr::to_string_lossy) else {
            debug!("Not hovering because no file name");
            return;
        };

        let text = format!("Play {file_name}");

        let font = TextStyle::Heading.resolve(&ctx.style());
        let painter = ctx.layer_painter(LayerId::new(
            Order::Foreground,
            Id::new("file_drop_preview"),
        ));
        let screen_rect = ctx.screen_rect();
        painter.rect_filled(screen_rect, 0., Color32::from_black_alpha(192));

        let mut galley = painter.layout(text, font, Color32::WHITE, screen_rect.width());
        center_galley(ctx, Arc::make_mut(&mut galley));
        let text_pos = (screen_rect.center() - galley.rect.center()).to_pos2();
        painter.galley(text_pos, galley, Color32::WHITE);
    }

    fn dnd_seed_dropped(&mut self, ctx: &Context) {
        let Some(Some(file_path)) = ctx.input(|i| {
            let dropped = &i.raw.dropped_files;
            (dropped.len() == 1).then(|| dropped[0].path.clone())
        }) else {
            return;
        };

        if !Self::valid_seed_file(&file_path) {
            return;
        }

        info!(?file_path, "Playing rando file");
        if let Err(err) = play_rando_file(&self.settings, file_path) {
            error!(?err, "Couldn't play rando file");
            self.error_message = Some("Failed to play seed".to_owned());
        }
    }

    fn valid_seed_file(file_path: &Path) -> bool {
        if !file_path.extension().is_some_and(|ext| ext == "dat") {
            debug!("Invalid seed file because of file extension");
            return false;
        }

        true
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
                    self.settings.theme_preference = ThemePreference::Light;
                }
            } else {
                if ui
                    .add(Button::new("🌙").frame(false))
                    .on_hover_text("Switch to dark mode")
                    .clicked()
                {
                    self.settings.theme_preference = ThemePreference::Dark;
                }
            }

            if ui.button("Launch game").clicked() {
                self.settings
                    .game_dir
                    .launch_game(self.settings.launch_type);
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
                            OriDllKind::Rando(v) => Some((dll, v)),
                            _ => None,
                        })
                        .max_by_key(|(_dll, v)| *v);

                    let has_unknown = all
                        .iter()
                        .any(|dll| matches!(dll.kind, OriDllKind::UnknownRando(_)));

                    match (newest_known, has_unknown) {
                        (Some((dll, v)), _) => InstalledState::Installed(v, dll.clone()),
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
                } else {
                    app.settings.stay_on_latest = true;
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
        style.visuals.selection.stroke.color = Color32::from_gray(15);
    });

    ctx.style_mut_of(Theme::Dark, |style| {
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(235);
        style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(235);
        style.visuals.selection.stroke.color = Color32::from_gray(245);
    });
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

fn center_galley(ctx: &Context, galley: &mut Galley) {
    let width = galley.rect.width();

    for row in &mut galley.rows {
        let row_width = row.rect.width();
        let offset = (width - row_width) * 0.5;

        let ppp = ctx.native_pixels_per_point().unwrap_or(1.);
        let offset = (offset * ppp).round() / ppp;

        if offset < 0.1 {
            continue;
        }

        row.rect.min.x += offset;
        row.rect.max.x += offset;

        for glyph in &mut row.glyphs {
            glyph.pos.x += offset;
        }

        for vertex in &mut row.visuals.mesh.vertices {
            vertex.pos.x += offset;
        }
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
