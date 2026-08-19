use crate::dll_classifier::RandoVersion;
use crate::dll_management::{OriDll, OriDllKind, install_new_dll, search_game_dir};
use crate::gui::game_settings::GameSettings;
use crate::orirando_website::{check_version, download_dll};
use crate::rando_files::play_rando_file;
use crate::settings::Settings;
use crate::{LOGFILE, StartupInfo};
use color_eyre::Result;
use eframe::NativeOptions;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, Context, Frame, Galley, IconData, Id, InnerResponse,
    LayerId, Layout, Margin, Modal, Order, Sense, Sides, Spinner, TextStyle, Theme,
    ThemePreference, Ui, UiBuilder, Vec2, ViewportBuilder, ViewportCommand, Widget,
};
use image::{ImageFormat, load_from_memory_with_format};
use std::ffi::OsStr;
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::thread;
use std::time::Duration;
use tracing::{Metadata, Span, debug, error, info, info_span, instrument, warn};
use winit::platform::windows::EventLoopBuilderExtWindows;

mod app_settings;
mod game_settings;
mod rando;
mod version_row;

#[derive(Clone)]
pub struct Gui {
    channel: Sender<GuiCommand>,
}

impl Gui {
    pub fn start(settings: Settings) -> Self {
        init_gui(settings)
    }

    pub fn show_timeout(&self, timeout: Duration) {
        let channel = self.channel.clone();
        thread::spawn(move || {
            thread::sleep(timeout);
            _ = channel.send(GuiCommand::Show);
        });
    }

    pub fn show_main_ui(&self, startup_info: StartupInfo) {
        _ = self.channel.send(GuiCommand::ShowMain(startup_info));
    }

    pub fn push_error(&self, message: &str) {
        _ = self.channel.send(GuiCommand::PushError(message.to_owned()));
    }

    pub fn show_error_ui(&self) {
        _ = self.channel.send(GuiCommand::ShowErrors);
    }

    pub fn wait(&self) {
        let (tx, rx) = mpsc::channel();
        _ = self.channel.send(GuiCommand::Wait(tx));
        _ = rx.recv();
    }
}

enum GuiCommand {
    Show,
    ShowErrors,
    ShowMain(StartupInfo),
    PushError(String),
    Wait(Sender<()>),
}
//noinspection RsUnwrap
#[instrument(skip(settings))]
fn init_gui(settings: Settings) -> Gui {
    let span = Span::current();

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let _entered = span.enter();
        let _entered = info_span!(parent: &span, "gui_thread").entered();

        gui_thread(settings, rx);
    });

    Gui { channel: tx }
}

#[allow(clippy::needless_pass_by_value)]
fn gui_thread(settings: Settings, command_rx: Receiver<GuiCommand>) {
    let span = Span::current();

    let (app_tx, app_rx) = mpsc::channel();
    let (start_tx, start_rx) = mpsc::channel();
    let (wait_tx, wait_rx) = mpsc::channel();

    thread::spawn(move || {
        let _entered = span.enter();
        let _entered_child = info_span!(parent: &span, "egui_thread").entered();

        let _wait_rx = wait_rx;

        egui_thread(settings, app_tx, start_rx);
    });

    let app = match app_rx.recv() {
        Ok(app) => app,
        Err(err) => {
            error!(?err, "GUI failed to start");
            return;
        }
    };

    while let Ok(cmd) = command_rx.recv() {
        match cmd {
            GuiCommand::Show => _ = start_tx.send(StartCommand::Start),
            GuiCommand::ShowMain(startup_info) => {
                let mut inner = app.inner.lock().unwrap();
                inner.display_mode = DisplayMode::Main;

                if let Some((current_dll, all_dlls)) = startup_info.dlls {
                    inner.newest_version_installed = Inner::get_installed_state(&all_dlls);
                    inner.current_dll = current_dll;
                    inner.just_updated = startup_info.updated;
                    inner.all_dlls = all_dlls;
                } else {
                    inner.update_dlls();
                }

                if let Some(latest) = startup_info.latest_rando_version {
                    inner.newest_version_available = NewestState::Version(latest);
                } else {
                    inner.check_newest();
                }

                inner.egui_ctx.request_repaint();
                _ = start_tx.send(StartCommand::Start);
            }
            GuiCommand::ShowErrors => {
                let mut inner = app.inner.lock().unwrap();
                if inner.error_messages.is_empty() {
                    _ = start_tx.send(StartCommand::Cancel);
                } else {
                    inner.display_mode = DisplayMode::Error;
                    inner.egui_ctx.request_repaint();
                    _ = start_tx.send(StartCommand::Start);
                }
            }
            GuiCommand::PushError(error) => {
                let mut inner = app.inner.lock().unwrap();
                inner.push_error(error);
                inner.egui_ctx.request_repaint();
            }
            GuiCommand::Wait(tx) => {
                _ = wait_tx.send(tx);
            }
        }
    }
}

enum StartCommand {
    Start,
    Cancel,
}

#[allow(clippy::needless_pass_by_value)]
fn egui_thread(
    settings: Settings,
    app_channel: Sender<App>,
    start_channel: Receiver<StartCommand>,
) {
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
            .with_inner_size([300., 300.])
            .with_icon(icon),

        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),

        ..Default::default()
    };

    let result = eframe::run_native(
        "Ori DE Randomizer",
        options,
        Box::new(|cc| {
            adjust_themes(&cc.egui_ctx);
            cc.egui_ctx.set_theme(settings.theme_preference);

            let app = App::new(settings, cc.egui_ctx.clone());

            app_channel
                .send(app.clone())
                .expect("Channel is valid if app hasn't crashed");

            if let Ok(StartCommand::Start) = start_channel.recv() {
                Ok(Box::new(app))
            } else {
                Err("Not supposed to start gui".into())
            }
        }),
    );

    if let Err(err) = result {
        error!(?err, "Error running gui");
    }
}

#[derive(Clone)]
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
        drop(inner);

        app
    }
}

#[derive(Default)]
struct Inner {
    weak_self: Weak<Mutex<Inner>>,
    egui_ctx: Context,
    display_mode: DisplayMode,
    show_settings: bool,
    settings: Settings,
    prev_settings: Settings,
    active_screen: ActiveScreen,
    current_dll: Option<OriDll>,
    all_dlls: Vec<OriDll>,
    just_updated: bool,
    newest_version_installed: InstalledState,
    newest_version_available: NewestState,
    modal_message: Option<String>,
    error_messages: Vec<String>,
    modal_uis: Vec<(AppModal, Box<DynModalUi>)>,
    game_settings: GameSettings,
}

#[derive(Default)]
enum DisplayMode {
    #[default]
    Loading,
    Main,
    Error,
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
        if let Some(msg) = self.error_messages.pop() {
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
                        ui.label(&msg);
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

            if !modal.inner && !modal.should_close() {
                self.error_messages.push(msg);
            }
        }
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.error_messages.push(message.into());
    }
}

impl Inner {
    fn render(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Ori DE Randomizer");
            });

            match self.display_mode {
                DisplayMode::Loading => Self::draw_loading_ui(ui),
                DisplayMode::Main => self.draw_main_ui(ui),
                DisplayMode::Error => self.draw_error_ui(),
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

    fn draw_loading_ui(ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            let height = ui.available_size().y;
            let spinner_size = f32::min(60., height);
            ui.allocate_exact_size(Vec2::new(0., (height - spinner_size) / 2.), Sense::hover());
            Spinner::new().size(spinner_size).ui(ui);
        });
    }

    fn draw_error_ui(&mut self) {
        if self.error_messages.is_empty() {
            self.egui_ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn draw_main_ui(&mut self, ui: &mut Ui) {
        let ctx = self.egui_ctx.clone();

        top_right(ui, |ui| {
            ui.toggle_value(&mut self.show_settings, "⛭")
                .on_hover_text("Settings");
        });

        if self.settings.game_dir.is_set() {
            self.dnd_seed(&ctx);

            if self.show_settings {
                self.draw_settings_ui(ui);
            } else {
                self.draw_rando_version(ui);
                if matches!(
                    self.newest_version_installed,
                    InstalledState::InstalledUnknown | InstalledState::Installed(..)
                ) {
                    self.draw_main_content(ui);
                }
            }
        } else {
            ui.label("Installation of Ori and the Blind Forest: Definitive Edition not found.");
            ui.label("Note: The randomizer is only compatible with the Definitive Edition, not the original.");
            ui.horizontal_wrapped(|ui| {
                ui.label("Please select the installation directory:");
                self.draw_choose_game_dir_button(ui);
            });
        }

        self.draw_bottom_row(ui);

        if let Some((mut modal, mut modal_ui)) = self.modal_uis.pop() {
            let resp = Modal::new(Id::new("ui_modal")).show(&ctx, |ui| {
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
            Modal::new(Id::new("modal message")).show(&ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(msg);
                    ui.spinner();
                });
            });
        }
    }

    fn draw_main_content(&mut self, ui: &mut Ui) {
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
            self.push_error("Failed to play seed");
        }
    }

    fn valid_seed_file(file_path: &Path) -> bool {
        if file_path.extension().is_none_or(|ext| ext != "dat") {
            debug!("Invalid seed file because of file extension");
            return false;
        }

        true
    }

    fn draw_show_log_button(ui: &mut Ui) {
        if let Some(path) = LOGFILE.get()
            && ui.button("Show logs").clicked()
        {
            if let Err(err) = opener::reveal(path) {
                error!(?err, "Couldn't show log file");
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
        S: FnOnce(&mut Self, R) + Send + 'static,
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

                let newest = Self::get_installed_state(&all);

                Some((current, all, newest))
            },
            |app, dlls| {
                let Some((current, all, newest)) = dlls else {
                    app.newest_version_installed = InstalledState::None;
                    app.push_error("Failed to load installed versions");
                    return;
                };

                info!("Updated dlls");
                app.current_dll = current;
                app.just_updated = true;
                app.all_dlls = all;
                app.newest_version_installed = newest;
            },
        );
    }

    fn get_installed_state(dlls: &[OriDll]) -> InstalledState {
        let newest_known = dlls
            .iter()
            .filter_map(|dll| match dll.kind {
                OriDllKind::Rando(v) => Some((dll, v)),
                _ => None,
            })
            .max_by_key(|(_dll, v)| *v);

        let has_unknown = dlls
            .iter()
            .any(|dll| matches!(dll.kind, OriDllKind::UnknownRando(_)));

        match (newest_known, has_unknown) {
            (Some((dll, v)), _) => InstalledState::Installed(v, dll.clone()),
            (None, true) => InstalledState::InstalledUnknown,
            _ => InstalledState::None,
        }
    }

    #[instrument(skip(self))]
    fn check_newest(&mut self) {
        self.newest_version_available = NewestState::Checking;

        let network = self.settings.network;
        info!("Checking for newest dll available");
        self.run_off_thread(
            move || match check_version(&network) {
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
        let network = self.settings.network;

        info!("Downloading update");
        self.run_off_thread(
            move || -> Result<()> {
                let dll = download_dll(&network)?;
                install_new_dll(&game_dir, &dll, &all_dlls)?;
                Ok(())
            },
            |app, result| {
                if let Err(err) = result {
                    error!(?err, "Error downloading update");
                    app.push_error("Failed to download update");
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
