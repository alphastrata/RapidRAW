use eframe::egui;

/// The darkroom chrome palette and the widget states it drives.
const CHROME: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const PANEL: egui::Color32 = egui::Color32::from_rgb(32, 32, 32);
const BUTTON: egui::Color32 = egui::Color32::from_rgb(45, 45, 45);
const HOVERED: egui::Color32 = egui::Color32::from_rgb(58, 58, 58);
const ACTIVE: egui::Color32 = egui::Color32::from_rgb(70, 70, 70);
const FIELD: egui::Color32 = egui::Color32::from_rgb(18, 18, 18);
const TEXT: egui::Color32 = egui::Color32::from_rgb(200, 200, 200);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(64, 132, 196);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = CHROME;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = PANEL;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, PANEL);
    craft_ui::theme::WidgetPalette {
        noninteractive: CHROME,
        inactive: BUTTON,
        hovered: HOVERED,
        active: ACTIVE,
        open: HOVERED,
    }
    .apply_to(
        &mut visuals.widgets,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, TEXT),
    );
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|styles| {
        styles.spacing.item_spacing = egui::vec2(6.0, 4.0);
        styles.spacing.button_padding = egui::vec2(8.0, 3.0);
        styles.spacing.interact_size.y = 22.0;
        styles.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(13.0),
        );
    });
}