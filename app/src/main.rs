#![recursion_limit = "256"]

mod editor;
mod host;
mod theme;
mod ui;

use eframe::egui;
use editor::Editor;
use host::{APP_ID, RapidHost};
use std::sync::mpsc;
use std::sync::Arc;

fn main() -> eframe::Result {
    env_logger::init();
    let host = Arc::new(RapidHost::new());
    let (pick_tx, picks) = std::sync::mpsc::channel();
    let pick_sender = pick_tx.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RapidRAW")
            .with_app_id(APP_ID)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(RapidRaw {
                editor: Editor::new(),
                host,
                open_requested: false,
                picks,
                pick_sender,
            }))
        }),
    )
}

struct RapidRaw {
    editor: Editor,
    host: Arc<RapidHost>,
    open_requested: bool,
    picks: mpsc::Receiver<Option<std::path::PathBuf>>,
    pick_sender: mpsc::Sender<Option<std::path::PathBuf>>,
}

impl RapidRaw {
    fn pump(&mut self) {
        while let Ok(pick) = self.picks.try_recv()
            && let Some(path) = pick
            && let Err(error) = self.editor.open(&self.host, &path)
        {
            log::error!("Failed to open {}: {error:#}", path.display());
        }
        if std::mem::take(&mut self.open_requested) {
            let tx = mpsc::Sender::clone(&self.pick_sender);
            std::thread::spawn(move || {
                let pick = rfd::FileDialog::new().set_title("Open Image").pick_file();
                tx.send(pick).ok();
            });
        }
    }
}

impl eframe::App for RapidRaw {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.editor.poll(&self.host, ui.ctx());
        self.pump();
        if ui.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.contains(egui::Modifiers::CTRL)) {
            self.open_requested = true;
        }
        ui::update(&mut self.editor, &self.host, &mut self.open_requested, ui);
        ui.ctx().request_repaint();
    }
}