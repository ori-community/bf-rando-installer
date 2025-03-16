use color_eyre::eyre::eyre;
use eframe::egui::{Align, Context, Layout, Theme, ViewportBuilder, ViewportCommand};
use eframe::{Frame, NativeOptions, egui};
use std::path::PathBuf;
use tracing::instrument;

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
            cc.egui_ctx.set_theme(Theme::Light);
            Ok(Box::new(App::new(ori_path)))
        }),
    );

    result.map_err(|e| eyre!("Error running gui: {e:?}"))
}

struct App {
    _ori_path: PathBuf,
    display_ori_path: String,
}

impl App {
    fn new(ori_path: PathBuf) -> App {
        let display_ori_path = ori_path.to_string_lossy().into_owned();
        Self {
            _ori_path: ori_path,
            display_ori_path,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Ori Rando Installer");
            });

            ui.horizontal(|ui| {
                ui.label("Ori Install Dir");
                ui.text_edit_singleline(&mut self.display_ori_path.as_str());
            });

            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });
    }
}
