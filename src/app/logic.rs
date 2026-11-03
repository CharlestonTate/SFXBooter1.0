use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;

use crate::app::{
    ClipEditHistory, ClipEntry, HotkeyActions, OpenClip, RenameState, SfxApp, WaveDrag,
};
use crate::audio::default_input;
use crate::audio::{
    AudioFormat, InputChoice, LoadedWav, Playback, Recorder, RegionEdit, edit_samples, read_clip,
    write_clip, write_recorded,
};
use crate::hotkeys::{Command, HotkeyManager};
use crate::storage::{
    ensure_clips_dir, list_clips, load_settings, new_clip_path, remove_from_sidecar,
    resolve_clips_dir, retarget_sidecar, same_clip_path, sanitize_stem, save_order, settings_path,
    write_settings,
};

impl SfxApp {
    pub(crate) fn new() -> Self {
        let settings_path = settings_path();
        let mut hotkeys = HotkeyManager::defaults();
        let (saved_mic, saved_name, saved_format, saved_clips_dir) =
            load_settings(&settings_path, &mut hotkeys);
        let (clips_dir, dir_error, clips_dir_input) = resolve_clips_dir(saved_clips_dir.as_deref());
        let format = saved_format.unwrap_or(AudioFormat::Wav);
        // a saved mic is trusted until record time, so a missing device still shows its old name
        let (mic_id, mic_name, input_error, settings_dirty) = if let Some(id) = saved_mic {
            (
                Some(id),
                saved_name.unwrap_or_else(|| "Microphone".into()),
                None,
                false,
            )
        } else {
            match default_input() {
                Ok(choice) => (Some(choice.id), choice.name, None, true),
                Err(err) => (None, String::new(), Some(err), false),
            }
        };
        let mut app = Self {
            clips_dir,
            settings_path,
            dir_error,
            clips: Vec::new(),
            open: None,
            rename: None,
            list_drag: None,
            list_drop_to: None,
            order_dirty: false,
            recorder: None,
            inputs: Vec::new(),
            mic_id,
            mic_name,
            input_error,
            input_list_error: None,
            format,
            hotkeys,
            settings_dirty,
            playback: None,
            play_from: 0,
            play_to: 0,
            play_started: None,
            meter: 0.0,
            status: None,
            wave_drag: WaveDrag::None,
            edit_history: HashMap::new(),
            clips_dir_input,
            queue: Vec::new(),
            queue_playing: false,
            queue_index: 0,
            playback_source: None,
            wave_zoom: 1.0,
            wave_pan: 0.5,
        };
        app.refresh_clips();
        app
    }

    pub(crate) fn refresh_clips(&mut self) {
        self.clips = list_clips(&self.clips_dir);
    }

    pub(crate) fn apply_clips_dir(&mut self) {
        let trimmed = self.clips_dir_input.trim();
        if trimmed.is_empty() {
            self.status = Some("Pick a folder path first.".into());
            return;
        }
        let path = PathBuf::from(trimmed);
        match ensure_clips_dir(&path) {
            Ok(dir) => {
                self.clips_dir = dir.clone();
                self.clips_dir_input = dir.display().to_string();
                self.dir_error = None;
                self.refresh_clips();
                self.settings_dirty = true;
                self.status = Some(format!("Using {}", self.clips_dir.display()));
            }
            Err(err) => {
                self.dir_error = Some(err.clone());
                self.status = Some(err);
            }
        }
    }

    pub(crate) fn push_undo(&mut self, path: &Path, wav: &LoadedWav) {
        let history = self.edit_history.entry(path.to_path_buf()).or_default();
        history.redo.clear();
        history.undo.push(wav.clone());
        if history.undo.len() > 40 {
            history.undo.remove(0);
        }
    }

    pub(crate) fn undo_edit(&mut self) {
        let Some(path) = self.open.as_ref().map(|open| open.path.clone()) else {
            self.status = Some("Open a clip to undo.".into());
            return;
        };
        let Some(snapshot) = self
            .edit_history
            .get_mut(&path)
            .and_then(|history| history.undo.pop())
        else {
            self.status = Some("Nothing to undo.".into());
            return;
        };
        if let Some(open) = &self.open {
            if let Some(history) = self.edit_history.get_mut(&path) {
                history.redo.push(open.wav.clone());
            }
        }
        if let Err(err) = self.restore_clip_snapshot(&path, &snapshot) {
            self.status = Some(err);
        } else {
            self.status = Some("Undo.".into());
        }
    }

    pub(crate) fn redo_edit(&mut self) {
        let Some(path) = self.open.as_ref().map(|open| open.path.clone()) else {
            self.status = Some("Open a clip to redo.".into());
            return;
        };
        let Some(snapshot) = self
            .edit_history
            .get_mut(&path)
            .and_then(|history| history.redo.pop())
        else {
            self.status = Some("Nothing to redo.".into());
            return;
        };
        if let Some(open) = &self.open {
            if let Some(history) = self.edit_history.get_mut(&path) {
                history.undo.push(open.wav.clone());
            }
        }
        if let Err(err) = self.restore_clip_snapshot(&path, &snapshot) {
            self.status = Some(err);
        } else {
            self.status = Some("Redo.".into());
        }
    }

    pub(crate) fn restore_clip_snapshot(
        &mut self,
        path: &Path,
        snapshot: &LoadedWav,
    ) -> Result<(), String> {
        let format = AudioFormat::from_path(path).unwrap_or(self.format);
        write_clip(
            path,
            snapshot.sample_rate,
            snapshot.channels,
            &snapshot.samples,
            format,
        )?;
        self.stop_playback();
        self.wave_drag = WaveDrag::None;
        self.open = None;
        self.open_clip(path);
        self.refresh_clips();
        Ok(())
    }

    pub(crate) fn add_to_queue(&mut self, path: PathBuf) {
        if self.queue.iter().any(|item| item == &path) {
            self.status = Some("Already in the queue.".into());
            return;
        }
        self.queue.push(path);
        self.status = None;
    }

    pub(crate) fn play_queue(&mut self) {
        if self.queue.is_empty() {
            self.status = Some("Queue is empty.".into());
            return;
        }
        self.queue_playing = true;
        self.queue_index = 0;
        self.play_queue_at(0);
    }

    pub(crate) fn play_queue_at(&mut self, index: usize) {
        let Some(path) = self.queue.get(index).cloned() else {
            self.queue_playing = false;
            self.playback_source = None;
            self.play_started = None;
            return;
        };
        let wav = match read_clip(&path) {
            Ok(wav) => wav,
            Err(err) => {
                self.status = Some(err);
                self.queue_playing = false;
                return;
            }
        };
        let frames = wav.frames();
        if frames == 0 {
            self.advance_queue_after_item();
            return;
        }
        if self.playback.is_none() {
            match Playback::open() {
                Ok(playback) => self.playback = Some(playback),
                Err(err) => {
                    self.status = Some(err);
                    self.queue_playing = false;
                    return;
                }
            }
        }
        if let Err(err) = self.playback.as_ref().unwrap().play(&wav, 0, frames) {
            self.status = Some(err);
            self.queue_playing = false;
            return;
        }
        self.playback_source = Some(wav);
        self.play_from = 0;
        self.play_to = frames;
        self.play_started = Some(Instant::now());
        self.status = Some(format!("Queue {}/{}", index + 1, self.queue.len()));
    }

    pub(crate) fn advance_queue_after_item(&mut self) {
        if !self.queue_playing {
            return;
        }
        let next = self.queue_index + 1;
        if next >= self.queue.len() {
            self.queue_playing = false;
            self.playback_source = None;
            self.play_started = None;
            self.status = Some("Queue finished.".into());
            return;
        }
        self.queue_index = next;
        self.play_queue_at(next);
    }

    pub(crate) fn tick_playback(&mut self) {
        if self.play_started.is_none() {
            return;
        }
        if self.playing() {
            return;
        }
        if self.queue_playing {
            self.advance_queue_after_item();
        } else {
            self.play_started = None;
            self.playback_source = None;
        }
    }

    pub(crate) fn start_recording(&mut self) {
        if self.dir_error.is_some() {
            return;
        }
        self.stop_playback();
        match Recorder::start(self.mic_id.as_deref()) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                self.meter = 0.0;
                self.status = None;
            }
            Err(err) => {
                self.input_error = Some(err.clone());
                self.status = Some(err);
            }
        }
    }

    pub(crate) fn stop_recording(&mut self) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        let recorded = recorder.finish();
        if recorded.samples.is_empty() {
            self.status = Some("Nothing recorded.".into());
            return;
        }
        let path = new_clip_path(&self.clips_dir, self.format);
        if let Err(err) = write_recorded(&path, &recorded, self.format) {
            self.status = Some(err);
            return;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("clip")
            .to_string();
        self.refresh_clips();
        self.open_clip(&path);
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            self.status = Some(format!("Saved {name}"));
        }
    }

    pub(crate) fn open_clip(&mut self, path: &Path) {
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            return;
        }
        // switching clips throws away a half typed name
        self.rename = None;
        self.stop_playback();
        self.wave_drag = WaveDrag::None;
        self.wave_zoom = 1.0;
        self.wave_pan = 0.5;
        match read_clip(path) {
            Ok(wav) => {
                self.open = Some(OpenClip {
                    path: path.to_path_buf(),
                    wav,
                    selection: None,
                });
                self.status = None;
            }
            Err(err) => self.status = Some(err),
        }
    }

    pub(crate) fn begin_rename(&mut self) {
        let Some(path) = self.open.as_ref().map(|open| open.path.clone()) else {
            self.status = Some("Open a clip to rename it.".into());
            return;
        };
        self.begin_rename_at(path);
    }

    // f2 or the list menu can land here. opens the clip first if it was not already up.
    pub(crate) fn begin_rename_at(&mut self, path: PathBuf) {
        if self.rename.is_some() {
            return;
        }
        if !self.open.as_ref().is_some_and(|open| open.path == path) {
            self.open_clip(&path);
        }
        if !self.open.as_ref().is_some_and(|open| open.path == path) {
            return;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("clip")
            .to_string();
        // the box edits the stem only. the extension stays with the format.
        let stem = match file_name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem.to_string(),
            _ => file_name,
        };
        self.rename = Some(RenameState {
            path,
            text: stem,
            focus: true,
        });
        self.status = None;
    }

    pub(crate) fn delete_clip(&mut self, path: &Path) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            self.stop_playback();
            self.open = None;
            self.wave_drag = WaveDrag::None;
        }
        if self
            .rename
            .as_ref()
            .is_some_and(|rename| rename.path == path)
        {
            self.rename = None;
        }
        if self.list_drag.as_deref() == Some(name) {
            self.list_drag = None;
            self.list_drop_to = None;
        }
        self.edit_history.remove(path);
        if let Err(err) = std::fs::remove_file(path) {
            self.status = Some(err.to_string());
            return;
        }
        remove_from_sidecar(&self.clips_dir, name);
        self.queue.retain(|queued| queued != path);
        self.refresh_clips();
        self.status = Some(format!("Deleted {name}"));
    }

    pub(crate) fn commit_rename(&mut self) {
        let Some(rename) = self.rename.clone() else {
            return;
        };
        // definately keep the extension the file already has
        let stem = sanitize_stem(&rename.text);
        if stem.is_empty() {
            self.status = Some("Type a name first.".into());
            if let Some(state) = self.rename.as_mut() {
                state.focus = true;
            }
            return;
        }
        let extension = rename
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("wav");
        let new_name = format!("{stem}.{extension}");
        let old_name = rename
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        if new_name == old_name {
            self.rename = None;
            return;
        }
        let dest = rename.path.with_file_name(&new_name);
        if dest.exists() && !same_clip_path(&dest, &rename.path) {
            self.status = Some("A clip with that name already exists.".into());
            if let Some(state) = self.rename.as_mut() {
                state.focus = true;
            }
            return;
        }
        if let Err(err) = std::fs::rename(&rename.path, &dest) {
            self.status = Some(err.to_string());
            return;
        }
        // move the saved date and the list slot onto the new filename
        retarget_sidecar(&self.clips_dir, &old_name, &new_name);
        if let Some(open) = self.open.as_mut() {
            if open.path == rename.path {
                open.path = dest.clone();
            }
        }
        if let Some(history) = self.edit_history.remove(&rename.path) {
            self.edit_history.insert(dest.clone(), history);
        }
        self.rename = None;
        self.refresh_clips();
        self.status = Some(format!("Renamed to {new_name}"));
    }

    pub(crate) fn play(&mut self, selection_only: bool) {
        self.queue_playing = false;
        self.playback_source = None;
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let frames = open.wav.frames();
        if frames == 0 {
            self.status = Some("Clip is empty.".into());
            return;
        }
        let (start, end) = if selection_only {
            match open.selection {
                Some((start, end)) if start < end => (start, end),
                _ => {
                    self.status = Some("Drag on the waveform to select a region.".into());
                    return;
                }
            }
        } else {
            (0, frames)
        };
        if self.playback.is_none() {
            match Playback::open() {
                Ok(playback) => self.playback = Some(playback),
                Err(err) => {
                    self.status = Some(err);
                    return;
                }
            }
        }
        let Some(open) = self.open.as_ref() else {
            return;
        };
        if let Err(err) = self.playback.as_ref().unwrap().play(&open.wav, start, end) {
            self.status = Some(err);
            return;
        }
        self.play_from = start;
        self.play_to = end;
        self.play_started = Some(Instant::now());
        self.status = None;
    }

    pub(crate) fn stop_playback(&mut self) {
        if let Some(playback) = &self.playback {
            playback.stop();
        }
        self.play_started = None;
        self.queue_playing = false;
        self.playback_source = None;
    }

    // saves the selected region as a new file next to the open clip (same folder).
    pub(crate) fn export_selection(&mut self) {
        let prepared = match self.open.as_ref() {
            None => return,
            Some(open) => match open.selection {
                None => Err("Drag on the waveform to select a region.".to_string()),
                Some((start, end)) => edit_samples(
                    &open.wav.samples,
                    open.wav.channels,
                    start,
                    end,
                    RegionEdit::Trim,
                )
                .map(|samples| {
                    (
                        open.path.clone(),
                        open.wav.sample_rate,
                        open.wav.channels,
                        samples,
                    )
                }),
            },
        };
        let (source_path, sample_rate, channels, samples) = match prepared {
            Ok(ready) => ready,
            Err(err) => {
                self.status = Some(err);
                return;
            }
        };
        let dir = source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.clips_dir.clone());
        let format = AudioFormat::from_path(&source_path).unwrap_or(self.format);
        let dest = new_clip_path(&dir, format);
        if let Err(err) = write_clip(&dest, sample_rate, channels, &samples, format) {
            self.status = Some(err);
            return;
        }
        let name = dest
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("clip")
            .to_string();
        self.refresh_clips();
        self.open_clip(&dest);
        self.status = Some(format!("Exported {name}"));
    }

    pub(crate) fn wave_view_range(&self, frames: usize) -> (usize, usize) {
        if frames == 0 {
            return (0, 0);
        }
        if self.wave_zoom <= 1.0 {
            return (0, frames);
        }
        let visible = ((frames as f32) / self.wave_zoom).ceil() as usize;
        let visible = visible.clamp(1, frames);
        let center = (self.wave_pan.clamp(0.0, 1.0) * frames as f32).round() as usize;
        let half = visible / 2;
        let mut start = center.saturating_sub(half);
        if start + visible > frames {
            start = frames.saturating_sub(visible);
        }
        (start, start + visible)
    }

    pub(crate) fn wave_zoom_with_factor(
        &mut self,
        frames: usize,
        pointer_x: f32,
        rect: egui::Rect,
        factor: f32,
    ) {
        if (factor - 1.0).abs() < f32::EPSILON || frames == 0 || rect.width() <= 0.0 {
            return;
        }
        let (view_start, view_end) = self.wave_view_range(frames);
        let view_frames = (view_end.saturating_sub(view_start).max(1)) as f32;
        let t = ((pointer_x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let frame_under = view_start as f32 + t * view_frames;

        self.wave_zoom = (self.wave_zoom * factor).clamp(1.0, 128.0);
        if self.wave_zoom <= 1.0 {
            self.wave_pan = 0.5;
            return;
        }
        let visible = ((frames as f32) / self.wave_zoom).ceil().max(1.0);
        let max_start = (frames as f32 - visible).max(0.0);
        let start = (frame_under - t * visible).clamp(0.0, max_start);
        let center = start + visible * 0.5;
        self.wave_pan = (center / frames as f32).clamp(0.0, 1.0);
    }

    pub(crate) fn wave_pan_by_scroll(&mut self, scroll_delta: f32, frames: usize) {
        if self.wave_zoom <= 1.0 || frames == 0 || scroll_delta.abs() < f32::EPSILON {
            return;
        }
        let visible = ((frames as f32) / self.wave_zoom).ceil().max(1.0);
        let max_start = (frames as f32 - visible).max(0.0);
        if max_start <= 0.0 {
            return;
        }
        let (view_start, _) = self.wave_view_range(frames);
        // scroll moves along the clip in units of the visible window, not a tiny fixed nudge
        let frame_delta = scroll_delta * 0.35 * (visible / 100.0);
        let start = (view_start as f32 + frame_delta).clamp(0.0, max_start);
        let center = start + visible * 0.5;
        self.wave_pan = (center / frames as f32).clamp(0.0, 1.0);
    }

    pub(crate) fn wave_pan_by_drag(&mut self, delta_x: f32, frames: usize, view_width: f32) {
        if self.wave_zoom <= 1.0 || frames == 0 || view_width <= 0.0 || delta_x.abs() < f32::EPSILON
        {
            return;
        }
        let visible = ((frames as f32) / self.wave_zoom).ceil().max(1.0);
        let max_start = (frames as f32 - visible).max(0.0);
        if max_start <= 0.0 {
            return;
        }
        let (view_start, _) = self.wave_view_range(frames);
        let frames_per_pixel = visible / view_width;
        let start = (view_start as f32 - delta_x * frames_per_pixel).clamp(0.0, max_start);
        let center = start + visible * 0.5;
        self.wave_pan = (center / frames as f32).clamp(0.0, 1.0);
    }

    pub(crate) fn apply_edit(&mut self, edit: RegionEdit) {
        let prepared = match self.open.as_ref() {
            None => return,
            Some(open) => match open.selection {
                None => Err("Drag on the waveform to select a region.".to_string()),
                Some((start, end)) => {
                    edit_samples(&open.wav.samples, open.wav.channels, start, end, edit).map(
                        |samples| {
                            (
                                open.path.clone(),
                                open.wav.clone(),
                                open.wav.sample_rate,
                                open.wav.channels,
                                samples,
                            )
                        },
                    )
                }
            },
        };
        let (path, before, sample_rate, channels, samples) = match prepared {
            Ok(ready) => ready,
            Err(err) => {
                self.status = Some(err);
                return;
            }
        };
        self.push_undo(&path, &before);
        // rewrite the same format the clip already is, so a cut does not turn flac into wav
        let format = AudioFormat::from_path(&path).unwrap_or(self.format);
        if let Err(err) = write_clip(&path, sample_rate, channels, &samples, format) {
            self.status = Some(err);
            return;
        }
        self.stop_playback();
        self.wave_drag = WaveDrag::None;
        self.open = None;
        self.open_clip(&path);
        self.refresh_clips();
        if self.open.is_some() {
            self.status = Some(match edit {
                RegionEdit::Trim => "Trimmed.".into(),
                RegionEdit::Cut => "Cut.".into(),
            });
        }
    }

    pub(crate) fn playback_wav(&self) -> Option<&LoadedWav> {
        if let Some(wav) = &self.playback_source {
            Some(wav)
        } else {
            self.open.as_ref().map(|open| &open.wav)
        }
    }

    pub(crate) fn playing(&self) -> bool {
        let Some(started) = self.play_started else {
            return false;
        };
        let Some(wav) = self.playback_wav() else {
            return false;
        };
        if wav.sample_rate == 0 || self.play_to <= self.play_from {
            return false;
        }
        let frame =
            self.play_from + (started.elapsed().as_secs_f32() * wav.sample_rate as f32) as usize;
        frame < self.play_to
    }

    pub(crate) fn playhead(&self) -> Option<usize> {
        if !self.playing() {
            return None;
        }
        let wav = self.playback_wav()?;
        let frame = self.play_from
            + (self.play_started?.elapsed().as_secs_f32() * wav.sample_rate as f32) as usize;
        Some(frame.min(wav.frames().saturating_sub(1)))
    }

    pub(crate) fn read_hotkeys(&self, ctx: &egui::Context) -> HotkeyActions {
        HotkeyActions {
            toggle_record: self.hotkeys.take(ctx, Command::ToggleRecord),
            play: self.hotkeys.take(ctx, Command::Play),
            play_selection: self.hotkeys.take(ctx, Command::PlaySelection),
            stop: self.hotkeys.take(ctx, Command::Stop),
            trim: self.hotkeys.take(ctx, Command::Trim),
            cut: self.hotkeys.take(ctx, Command::Cut),
            rename: self.hotkeys.take(ctx, Command::Rename),
            show_hotkeys: self.hotkeys.take(ctx, Command::ShowHotkeys),
        }
    }

    pub(crate) fn save_settings_if_dirty(&mut self) {
        if !self.settings_dirty && !self.hotkeys.dirty {
            return;
        }
        match write_settings(
            &self.settings_path,
            self.mic_id.as_deref(),
            &self.mic_name,
            self.format,
            &self.clips_dir,
            &self.hotkeys,
        ) {
            Ok(()) => {
                self.settings_dirty = false;
                self.hotkeys.dirty = false;
            }
            Err(err) => {
                if self.status.is_none() {
                    self.status = Some(err);
                }
            }
        }
    }
}
