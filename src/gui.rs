use color_eyre::eyre::eyre;
use eframe::NativeOptions;
use eframe::egui::{
    Align, CentralPanel, Color32, ComboBox, Context, Id, Layout, Modal, Theme, ViewportBuilder,
    ViewportCommand,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use tracing::{error, info, instrument, warn};

use crate::dll_management::{GameDir, OriDll, OriDllKind, install_dll, search_game_dir};
use color_eyre::Result;

#[instrument]
pub fn run_gui(ori_path: PathBuf) -> Result<()> {
    let options = NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([300., 200.]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Ori Rando Installer",
        options,
        Box::new(|cc| {
            adjust_themes(&cc.egui_ctx);
            Ok(Box::new(App::new(ori_path)))
        }),
    );

    result.map_err(|e| eyre!("Error running gui: {e:?}"))
}

struct App {
    inner: Arc<Mutex<Inner>>,
}

impl App {
    fn new(ori_path: PathBuf) -> App {
        let display_ori_path = ori_path.to_string_lossy().into_owned();
        let this = Self {
            inner: Arc::new(Mutex::new(Inner {
                weak_self: Default::default(),
                _ori_path: ori_path.clone(),
                game_dir: GameDir::new(ori_path),
                display_ori_path,
                current_dll: None,
                all_dlls: Vec::new(),
                updating_dlls: false,
                installing_dll: false,
            })),
        };

        let mut inner = this.inner.lock().unwrap();
        inner.weak_self = Arc::downgrade(&this.inner);
        inner.update_dlls();
        drop(inner);

        this
    }
}

struct Inner {
    weak_self: Weak<Mutex<Inner>>,
    _ori_path: PathBuf,
    game_dir: GameDir,
    display_ori_path: String,
    current_dll: Option<OriDll>,
    all_dlls: Vec<OriDll>,
    updating_dlls: bool,
    installing_dll: bool,
}

impl eframe::App for App {
    #[instrument(skip(self, ctx, _frame))]
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut inner = self.inner.lock().unwrap();

        CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Ori Rando Installer");
            });

            ui.horizontal(|ui| {
                ui.label("Ori Install dir");
                ui.text_edit_singleline(&mut inner.display_ori_path.as_str());
            });

            ui.horizontal(|ui| {
                if inner.updating_dlls {
                    ui.label("Loading versions...");
                    ui.spinner();
                } else {
                    ui.label("Switch version");

                    ComboBox::from_id_salt("Select version CB")
                        .selected_text(format_dll(&inner.current_dll))
                        .show_ui(ui, |ui| {
                            let mut new_version = inner.current_dll.clone();
                            for dll in inner.all_dlls.iter().cloned().map(Some) {
                                let label = format_dll(&dll);
                                ui.selectable_value(&mut new_version, dll, label);
                            }

                            if new_version != inner.current_dll {
                                if let Some(version) = new_version {
                                    inner.switch_to_version(version);
                                } else {
                                    error!("Selected <none> version. This shouldn't be possible (doing nothing)");
                                }
                            }
                        });
                }
            });

            if inner.installing_dll {
                Modal::new(Id::new("installing dll modal")).show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label("Switching version...");
                        ui.spinner();
                    });
                });
            }

            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });
    }
}

impl Inner {
    fn run_off_thread<C, S, R>(&self, calc: C, sync: S)
    where
        C: (FnOnce() -> R) + Send + 'static,
        S: (FnOnce(&mut Self, R)) + Send + 'static,
    {
        let weak_self = self.weak_self.clone();

        thread::spawn(move || {
            let value = calc();

            if let Some(app) = weak_self.upgrade() {
                let mut app = app.lock().unwrap();
                sync(&mut app, value);
            }
        });
    }

    #[instrument(skip(self))]
    fn update_dlls(&mut self) {
        if self.updating_dlls {
            warn!("Tried to update dlls, while an update is already in progress. Aborting.");
            return;
        }

        info!("Updating dlls...");

        self.updating_dlls = true;

        let game_dir = self.game_dir.clone();
        self.run_off_thread(
            move || search_game_dir(&game_dir),
            |app, dlls| {
                match dlls {
                    Ok((current, all)) => {
                        info!("Updated dlls");
                        app.current_dll = current;
                        app.all_dlls = all;
                    }
                    Err(e) => {
                        error!(?e, "Couldn't update dlls");
                    }
                };
                app.updating_dlls = false;
            },
        );
    }

    #[instrument(skip(self, version))]
    fn switch_to_version(&mut self, version: OriDll) {
        info!(to_install=?version, "Switching version");

        self.installing_dll = true;

        let game_dir = self.game_dir.clone();
        let all_dlls = self.all_dlls.clone();

        self.run_off_thread(
            move || {
                if let Err(e) = install_dll(&game_dir, &version, &all_dlls) {
                    error!(?version, ?e, "Couldn't install new dll");
                }
            },
            |app, _| {
                app.installing_dll = false;
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
