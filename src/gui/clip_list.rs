use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app::{HotkeyActions, SfxApp, WaveDrag};
use crate::audio::{RegionEdit, build_peaks};
use crate::gui::format::{channel_label, format_clock, format_rate, format_secs, frame_secs};
use crate::hotkeys::Command;
use crate::storage::{
    list_visual_slot, move_clip, open_folder_in_explorer, reveal_in_explorer, save_order,
};

impl SfxApp {
    pub(crate) fn show_list(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        const ROW_H: f32 = 58.0;

        ui.heading("Clips");
        ui.label(
            egui::RichText::new(self.clips_dir.display().to_string())
                .small()
                .weak(),
        );
        ui.separator();
        ui.weak("Drag the grip to reorder. Right-click a clip for more.");

        let mut open_path = None;
        let mut rename_path = None;
        let mut delete_path = None;
        let mut queue_add_path = None;
        let mut explorer_error = None;
        let opened = self.open.as_ref().map(|clip| clip.path.clone());
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let released = ui.input(|input| input.pointer.any_released());

        let mut ghost: Option<(String, String, f32, PathBuf)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.clips.is_empty() {
                    ui.weak("No clips yet.");
                    return;
                }

                let drag_from = self
                    .list_drag
                    .as_ref()
                    .and_then(|name| self.clips.iter().position(|clip| clip.name == *name));
                let drag_to = self.list_drop_to.or(drag_from);
                let count = self.clips.len();
                ui.set_min_height(count as f32 * ROW_H);
                let origin = ui.cursor().min;
                let row_width = ui.available_width();

                for index in 0..count {
                    let name = self.clips[index].name.clone();
                    let path = self.clips[index].path.clone();
                    let date = self.clips[index].recorded_label.clone();
                    let duration = self.clips[index].duration;
                    let selected = opened.as_ref() == Some(&path);
                    let dragging = drag_from == Some(index);

                    if dragging {
                        ghost = Some((name.clone(), date.clone(), duration, path.clone()));
                    }

                    let slot = list_visual_slot(index, drag_from, drag_to);
                    let target_y = slot as f32 * ROW_H;
                    let animated_y = ui.ctx().animate_value_with_time(
                        ui.id().with(("clip_row_y", index)),
                        target_y,
                        0.14,
                    );
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(origin.x, origin.y + animated_y),
                        egui::vec2(row_width, 56.0),
                    );

                    let row_id = ui.id().with(("clip_row", index));
                    let row_response = ui.interact(row_rect, row_id, egui::Sense::click());
                    let grip = egui::Rect::from_min_size(
                        row_rect.min,
                        egui::vec2(22.0, row_rect.height()),
                    );
                    let grip_response = ui
                        .interact(
                            grip,
                            ui.id().with(("clip-grip", &name)),
                            egui::Sense::drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    if grip_response.drag_started() {
                        self.list_drag = Some(name.clone());
                        self.list_drop_to = Some(index);
                    }

                    let weak = ui.visuals().weak_text_color();
                    let mut visuals = ui
                        .style()
                        .interact_selectable(&row_response, selected || dragging);
                    if dragging {
                        visuals.bg_fill = visuals.bg_fill.gamma_multiply(0.55);
                    }
                    let fill = visuals.bg_fill;
                    let text_color = visuals.text_color();
                    let painter = ui.painter();
                    painter.rect_filled(row_rect, 4.0, fill);
                    let grip_color = if dragging { text_color } else { weak };
                    let grip_span = grip.shrink2(egui::vec2(0.0, 16.0)).y_range();
                    painter.vline(
                        grip.center().x - 2.0,
                        grip_span,
                        egui::Stroke::new(2.0, grip_color),
                    );
                    painter.vline(
                        grip.center().x + 2.0,
                        grip_span,
                        egui::Stroke::new(2.0, grip_color),
                    );
                    painter.text(
                        row_rect.left_top() + egui::vec2(28.0, 8.0),
                        egui::Align2::LEFT_TOP,
                        &name,
                        egui::FontId::proportional(14.0),
                        text_color,
                    );
                    painter.text(
                        row_rect.left_top() + egui::vec2(28.0, 30.0),
                        egui::Align2::LEFT_TOP,
                        format!("{date}  ·  {}", format_secs(duration)),
                        egui::FontId::proportional(12.0),
                        weak,
                    );

                    if row_response.clicked() {
                        if let Some(pos) = row_response.interact_pointer_pos() {
                            if pos.x >= row_rect.left() + 22.0 && self.list_drag.is_none() {
                                open_path = Some(path.clone());
                            }
                        }
                    }
                    let path_for_menu = path.clone();
                    row_response.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            rename_path = Some(path_for_menu.clone());
                        }
                        if ui.button("Add to queue").clicked() {
                            queue_add_path = Some(path_for_menu.clone());
                        }
                        if ui.button("Show in Explorer").clicked() {
                            if let Err(err) = reveal_in_explorer(&path_for_menu) {
                                explorer_error = Some(err);
                            }
                        }
                        if ui.button("Delete").clicked() {
                            delete_path = Some(path_for_menu);
                        }
                    });
                }

                ui.allocate_space(egui::vec2(0.0, count as f32 * ROW_H));

                if self.list_drag.is_some() {
                    if let Some(pos) = pointer {
                        let hover = (0..count).find(|index| {
                            let slot = list_visual_slot(*index, drag_from, drag_to);
                            let top = ui.ctx().animate_value_with_time(
                                ui.id().with(("clip_row_y", *index)),
                                slot as f32 * ROW_H,
                                0.14,
                            );
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(origin.x, origin.y + top),
                                egui::vec2(row_width, 56.0),
                            );
                            rect.contains(pos)
                        });
                        if let Some(to) = hover {
                            self.list_drop_to = Some(to);
                        }
                    }
                }
            });

        if let Some((name, date, duration, path)) = ghost {
            if let Some(pos) = pointer {
                let ghost_rect = egui::Rect::from_center_size(pos, egui::vec2(280.0, 56.0));
                let layer =
                    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("clip_drag_ghost"));
                let painter = ui.ctx().layer_painter(layer);
                painter.rect_filled(
                    ghost_rect,
                    6.0,
                    egui::Color32::from_rgba_unmultiplied(70, 120, 200, 220),
                );
                painter.rect_stroke(
                    ghost_rect,
                    6.0,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 180, 255)),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    ghost_rect.left_top() + egui::vec2(28.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                );
                painter.text(
                    ghost_rect.left_top() + egui::vec2(28.0, 30.0),
                    egui::Align2::LEFT_TOP,
                    format!("{date}  ·  {}", format_secs(duration)),
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_white_alpha(200),
                );
                let _ = path;
            }
        }

        if released {
            if let Some(name) = self.list_drag.take() {
                if let (Some(from), Some(to)) = (
                    self.clips.iter().position(|clip| clip.name == name),
                    self.list_drop_to.take(),
                ) {
                    if from != to {
                        move_clip(&mut self.clips, from, to);
                        save_order(&self.clips_dir, &self.clips);
                    }
                }
                self.list_drop_to = None;
            }
        }
        if self.list_drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            ui.ctx().request_repaint();
        }
        if let Some(path) = rename_path {
            self.begin_rename_at(path);
        }
        if let Some(path) = delete_path {
            self.delete_clip(&path);
        }
        if let Some(path) = queue_add_path {
            self.add_to_queue(path);
        }
        if let Some(err) = explorer_error {
            self.status = Some(err);
        }
        open_path
    }
}
