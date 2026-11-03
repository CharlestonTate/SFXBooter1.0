use std::path::{Path, PathBuf};

use eframe::egui;

use crate::app::{HotkeyActions, SfxApp, WaveDrag};
use crate::audio::{AudioFormat, list_inputs};
use crate::gui::format::{channel_label, format_clock, format_rate, format_secs, frame_secs};
use crate::gui::widgets::level_meter;
use crate::hotkeys::Command;
use crate::storage::{
    list_visual_slot, move_clip, open_folder_in_explorer, reveal_in_explorer, save_order,
};

impl SfxApp {
    pub(crate) fn microphone_menu(&mut self, ui: &mut egui::Ui) {
        let label = if self.mic_name.is_empty() {
            "Choose microphone".to_string()
        } else {
            self.mic_name.clone()
        };
        let mic_button = ui.add_sized([240.0, 24.0], egui::Button::new(label).truncate());
        if mic_button.clicked() {
            match list_inputs() {
                Ok(inputs) => {
                    self.inputs = inputs;
                    self.input_list_error = None;
                }
                Err(err) => {
                    self.inputs.clear();
                    self.input_list_error = Some(err);
                }
            }
        }
        let inputs = self.inputs.clone();
        let current = self.mic_id.clone();
        let list_error = self.input_list_error.clone();
        let mut picked = None;
        egui::Popup::menu(&mic_button).show(|ui| {
            ui.set_min_width(240.0);
            if let Some(err) = &list_error {
                ui.colored_label(egui::Color32::from_rgb(220, 150, 70), err);
            } else if inputs.is_empty() {
                ui.weak("No microphones found.");
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for choice in &inputs {
                        let selected = current.as_deref() == Some(choice.id.as_str());
                        if ui.selectable_label(selected, &choice.name).clicked() {
                            picked = Some(choice.clone());
                        }
                    }
                });
        });
        if let Some(choice) = picked {
            self.mic_id = Some(choice.id);
            self.mic_name = choice.name;
            self.input_error = None;
            self.settings_dirty = true;
        }
    }

    pub(crate) fn format_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::ComboBox::from_id_salt("save_format")
            .selected_text(self.format.label())
            .show_ui(ui, |ui| {
                for choice in AudioFormat::ALL {
                    if ui
                        .selectable_value(&mut self.format, choice, choice.label())
                        .changed()
                    {
                        changed = true;
                    }
                }
            });
        if changed {
            self.settings_dirty = true;
        }
    }

    pub(crate) fn show_folder_bar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clips folder");
        ui.add(
            egui::TextEdit::singleline(&mut self.clips_dir_input)
                .hint_text("folder for your sound files")
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if ui.button("Browse…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.clips_dir_input = path.display().to_string();
                }
            }
            if ui.button("Apply").clicked() {
                self.apply_clips_dir();
            }
            if ui.button("Open folder").clicked() {
                if let Err(err) = open_folder_in_explorer(&self.clips_dir) {
                    self.status = Some(err);
                }
            }
        });
        ui.weak("WAV, AIFF, FLAC, and MP3 in this folder appear below. Record saves here too.");
    }

    pub(crate) fn show_queue(&mut self, ui: &mut egui::Ui) {
        ui.heading("Queue");
        ui.horizontal_wrapped(|ui| {
            let can_add = self.open.is_some();
            if ui
                .add_enabled(can_add, egui::Button::new("Add open"))
                .clicked()
            {
                if let Some(path) = self.open.as_ref().map(|open| open.path.clone()) {
                    self.add_to_queue(path);
                }
            }
            if ui
                .add_enabled(!self.queue.is_empty(), egui::Button::new("Play queue"))
                .clicked()
            {
                self.play_queue();
            }
            if ui
                .add_enabled(!self.queue.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                self.queue.clear();
                if self.queue_playing {
                    self.stop_playback();
                }
            }
        });
        ui.separator();
        if self.queue.is_empty() {
            ui.weak("Right-click a clip or use Add open.");
            return;
        }
        let mut remove_at = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, path) in self.queue.iter().enumerate() {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("clip");
                    ui.horizontal(|ui| {
                        ui.label(format!("{}. {name}", index + 1));
                        if ui.small_button("Remove").clicked() {
                            remove_at = Some(index);
                        }
                    });
                }
            });
        if let Some(index) = remove_at {
            self.queue.remove(index);
            if self.queue_playing && index <= self.queue_index {
                self.queue_index = self.queue_index.saturating_sub(1);
            }
        }
    }
}
