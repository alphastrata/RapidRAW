use crate::editor::Editor;
use crate::host::RapidHost;
use eframe::egui;
use std::sync::Arc;

/// One frame: the Chrome-style layout is panels chained from the root `Ui`.
pub fn update(app: &mut Editor, _host: &Arc<RapidHost>, open: &mut bool, ui: &mut egui::Ui) {
    top_bar(app, open, ui);
    develop_panel(app, ui);
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(10, 10, 10)))
        .show(ui, |ui| draw_canvas(app, ui));
}

fn top_bar(app: &mut Editor, open: &mut bool, ui: &mut egui::Ui) {
    egui::Panel::top("top_bar")
        .default_size(38.0)
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(20, 20, 20)).inner_margin(8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").clicked() {
                    *open = true;
                }
                if let Some(file) = &app.loaded {
                    ui.separator();
                    ui.label(&file.file_name);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.rendering() {
                        ui.weak("rendering…");
                    }
                });
            });
        });
}

fn develop_panel(app: &mut Editor, ui: &mut egui::Ui) {
    egui::Panel::right("develop_panel")
        .resizable(true)
        .default_size(300.0)
        .min_size(240.0)
        .max_size(420.0)
        .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(30, 30, 30)).inner_margin(12.0))
        .show(ui, |ui| {
            ui.heading("Develop");
            ui.separator();
            if app.loaded.is_none() {
                ui.label("Open an image to develop.");
                return;
            }
            egui::CollapsingHeader::new("Basic")
                .default_open(true)
                .show(ui, |ui| develop_basic(ui, app));
            egui::CollapsingHeader::new("Detail")
                .default_open(true)
                .show(ui, |ui| {
                    slider(ui, app, "Sharpness", 0.0, 100.0, 1.0, "sharpness");
                });
            egui::CollapsingHeader::new("Presence")
                .default_open(true)
                .show(ui, |ui| {
                    slider(ui, app, "Clarity", -100.0, 100.0, 1.0, "clarity");
                    slider(ui, app, "Dehaze", -100.0, 100.0, 1.0, "dehaze");
                });
        });
}

fn develop_basic(ui: &mut egui::Ui, app: &mut Editor) {
    slider(ui, app, "Exposure", -5.0, 5.0, 0.01, "exposure");
    slider(ui, app, "Contrast", -100.0, 100.0, 1.0, "contrast");
    slider(ui, app, "Highlights", -100.0, 100.0, 1.0, "highlights");
    slider(ui, app, "Shadows", -100.0, 100.0, 1.0, "shadows");
    slider(ui, app, "Whites", -100.0, 100.0, 1.0, "whites");
    slider(ui, app, "Blacks", -100.0, 100.0, 1.0, "blacks");
    ui.separator();
    slider(ui, app, "Vibrance", -100.0, 100.0, 1.0, "vibrance");
    slider(ui, app, "Saturation", -100.0, 100.0, 1.0, "saturation");
}

/// A labelled, resettable development slider bound to one flat adjustment key.
fn slider(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    label: &str,
    min: f64,
    max: f64,
    step: f64,
    key: &'static str,
) {
    let Some(file) = editor.loaded.as_ref() else { return };
    let mut shown = file.adjustments[key].as_f64().unwrap_or(0.0);
    let changed = ui
        .horizontal(|ui| {
            ui.monospace(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let reset = ui.small_button("↺").clicked();
                let dragged = ui
                    .add(egui::Slider::new(&mut shown, min..=max).step_by(step))
                    .changed();
                reset || dragged
            })
            .inner
        })
        .inner;
    if changed {
        editor.adjust(key, shown);
    }
}

fn draw_canvas(app: &Editor, ui: &mut egui::Ui) {
    let Some(texture) = app.loaded.as_ref().and_then(|file| file.texture.as_ref()) else {
        if app.loaded.is_none() {
            ui.centered_and_justified(|ui| ui.weak("Open an image (Ctrl+O) to begin."));
        }
        return;
    };
    let size_vec = texture.size_vec2();
    let available = ui.available_size();
    let (image_w, image_h) = (size_vec.x, size_vec.y);
    let fit = if image_w > 0.0 && image_h > 0.0 { (available.x / image_w).min(available.y / image_h) } else { 1.0 };
    let size = egui::vec2(image_w * fit, image_h * fit);
    let viewport = ui.available_rect_before_wrap();
    let origin = egui::pos2(
        viewport.left() + (viewport.width() - size.x) * 0.5,
        viewport.top() + (viewport.height() - size.y) * 0.5,
    );
    egui::Image::new(texture)
        .fit_to_exact_size(size)
        .paint_at(ui, egui::Rect::from_min_size(origin, size));
}