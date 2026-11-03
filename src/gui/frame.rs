use eframe::egui;

use crate::app::{HotkeyActions, SfxApp};
use crate::audio::RegionEdit;
use crate::gui::format::format_clock;
use crate::gui::widgets::level_meter;
use crate::hotkeys::Command;
use crate::storage::save_order;

impl eframe::App for SfxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.recorder.is_some()
            || self.playing()
            || self.play_started.is_some()
            || self.list_drag.is_some()
            || self.list_drop_to.is_some()
        {
            ui.ctx().request_repaint();
        }

        // read keys before the widgets are built, or a focused button would click too.
        // skipped while typing, becuase the letters belong to the new file name.
        // only a text box should swallow keys. a focused button still leaves hotkeys free.
        let typing = ui.ctx().text_edit_focused();
        let capturing = self.hotkeys.poll_capture(ui.ctx());
        let keys = if typing || capturing {
            HotkeyActions::default()
        } else {
            self.read_hotkeys(ui.ctx())
        };
        if !typing && !capturing {
            if ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z))
            {
                self.undo_edit();
            } else if ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Y))
            {
                self.redo_edit();
            }
        }
        if keys.rename {
            self.begin_rename();
        }
        if keys.show_hotkeys {
            self.hotkeys.open = !self.hotkeys.open;
        }

        let recording = self.recorder.is_some();
        let elapsed = self
            .recorder
            .as_ref()
            .map(|recorder| recorder.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let level = self
            .recorder
            .as_ref()
            .map(|recorder| recorder.level())
            .unwrap_or(0.0);
        if recording {
            self.meter = self.meter.max(level) * 0.86 + level * 0.14;
        } else {
            self.meter *= 0.8;
        }

        let mut start_record = keys.toggle_record && !recording;
        let mut stop_record = keys.toggle_record && recording;
        let play_all = keys.play;
        let play_selection = keys.play_selection;
        let stop_playback = keys.stop;
        let trim = keys.trim;
        let cut = keys.cut;
        let input_error = self.input_error.clone();
        let dir_error = self.dir_error.clone();
        let status = self.status.clone();
        let meter = self.meter;
        let record_key = self.hotkeys.label(Command::ToggleRecord);

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SFX Record");
                ui.separator();
                if recording {
                    let stop = ui
                        .add(egui::Button::new("Stop").fill(egui::Color32::from_rgb(170, 48, 48)));
                    if stop.clicked() {
                        stop_record = true;
                    }
                    stop.on_hover_text(format!("{record_key} stops recording"));
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "Recording");
                    ui.monospace(format_clock(elapsed));
                    level_meter(ui, meter);
                } else {
                    let can_record = input_error.is_none() && dir_error.is_none();
                    let record = ui.add_enabled(
                        can_record,
                        egui::Button::new("Record").fill(egui::Color32::from_rgb(176, 48, 48)),
                    );
                    if record.clicked() {
                        start_record = true;
                    }
                    record.on_hover_text(format!("{record_key} starts recording"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Hotkeys").clicked() {
                        self.hotkeys.open = true;
                    }
                });
            });
            // mic and format stay put while a take is running, so the file matches the click
            ui.add_enabled_ui(!recording, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Microphone");
                    self.microphone_menu(ui);
                    ui.separator();
                    ui.label("Format");
                    self.format_menu(ui);
                });
            });
            if status.is_none() {
                if let Some(err) = dir_error.as_ref().or(input_error.as_ref()) {
                    if !recording {
                        ui.colored_label(egui::Color32::from_rgb(220, 150, 70), err);
                    }
                }
            }
            if let Some(status) = &status {
                ui.weak(status);
            }
        });

        let mut open_path = None;
        egui::Panel::left("clips")
            .default_size(300.0)
            .show(ui, |ui| {
                self.show_folder_bar(ui);
                ui.separator();
                open_path = self.show_list(ui);
            });

        egui::Panel::right("queue")
            .default_size(240.0)
            .show(ui, |ui| {
                self.show_queue(ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.show_clip(ui);
        });

        // the manager is a normal window. closing it only hides the list of keys.
        let mut hotkeys_open = self.hotkeys.open;
        egui::Window::new("Hotkeys")
            .open(&mut hotkeys_open)
            .default_size([460.0, 320.0])
            .show(ui.ctx(), |ui| {
                self.hotkeys.ui(ui);
            });
        self.hotkeys.open = hotkeys_open;

        if self.recorder.is_some() && stop_record {
            self.stop_recording();
        } else if self.recorder.is_none() && start_record {
            self.start_recording();
        }
        if let Some(path) = open_path {
            self.open_clip(&path);
        }
        if stop_playback {
            self.stop_playback();
        }
        if play_selection {
            self.play(true);
        } else if play_all {
            self.play(false);
        }
        if trim {
            self.apply_edit(RegionEdit::Trim);
        } else if cut {
            self.apply_edit(RegionEdit::Cut);
        }
        self.tick_playback();
        self.save_settings_if_dirty();
    }

    fn on_exit(&mut self) {
        if self.recorder.is_some() {
            self.stop_recording();
        }
        if self.order_dirty {
            save_order(&self.clips_dir, &self.clips);
            self.order_dirty = false;
        }
        self.save_settings_if_dirty();
    }
}
