use eframe::egui;

pub(crate) fn level_meter(ui: &mut egui::Ui, level: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 16.0), egui::Sense::hover());
    let bg = ui.visuals().extreme_bg_color;
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, bg);
    let width = rect.width() * level.clamp(0.0, 1.0);
    let color = if level > 0.92 {
        egui::Color32::from_rgb(220, 64, 64)
    } else {
        egui::Color32::from_rgb(64, 186, 108)
    };
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(width, rect.height())),
        3.0,
        color,
    );
}
