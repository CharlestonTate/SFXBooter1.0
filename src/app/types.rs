use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use crate::audio::{AudioFormat, InputChoice, LoadedWav, Playback, Recorder};
use crate::hotkeys::HotkeyManager;

pub(crate) struct ClipEntry {
    pub path: PathBuf,
    pub name: String,
    pub duration: f32,
    // the moment it was recorded, kept even after a later trim rewrites the file
    pub recorded: SystemTime,
    pub recorded_label: String,
}

pub(crate) struct OpenClip {
    pub path: PathBuf,
    pub wav: LoadedWav,
    pub selection: Option<(usize, usize)>,
}

// f2 swaps the title for a text box. focus is only requested on the first frame.
#[derive(Clone)]
pub(crate) struct RenameState {
    pub path: PathBuf,
    pub text: String,
    pub focus: bool,
}

pub(crate) enum WaveDrag {
    None,
    Select { anchor: usize },
    Start,
    End,
}

// undo/redo stacks for trim and cut on each file path
#[derive(Default)]
pub(crate) struct ClipEditHistory {
    pub undo: Vec<LoadedWav>,
    pub redo: Vec<LoadedWav>,
}

// hotkey results for this frame. buttons fill the same flags.
#[derive(Default)]
pub(crate) struct HotkeyActions {
    pub toggle_record: bool,
    pub play: bool,
    pub play_selection: bool,
    pub stop: bool,
    pub trim: bool,
    pub cut: bool,
    pub rename: bool,
    pub show_hotkeys: bool,
}

pub(crate) struct SfxApp {
    pub clips_dir: PathBuf,
    pub settings_path: PathBuf,
    pub dir_error: Option<String>,
    pub clips: Vec<ClipEntry>,
    pub open: Option<OpenClip>,
    pub rename: Option<RenameState>,
    pub list_drag: Option<String>,
    pub list_drop_to: Option<usize>,
    pub order_dirty: bool,
    pub recorder: Option<Recorder>,
    pub inputs: Vec<InputChoice>,
    pub mic_id: Option<String>,
    pub mic_name: String,
    pub input_error: Option<String>,
    pub input_list_error: Option<String>,
    pub format: AudioFormat,
    pub hotkeys: HotkeyManager,
    pub settings_dirty: bool,
    pub playback: Option<Playback>,
    pub play_from: usize,
    pub play_to: usize,
    pub play_started: Option<Instant>,
    pub meter: f32,
    pub status: Option<String>,
    pub wave_drag: WaveDrag,
    pub edit_history: HashMap<PathBuf, ClipEditHistory>,
    pub clips_dir_input: String,
    pub queue: Vec<PathBuf>,
    pub queue_playing: bool,
    pub queue_index: usize,
    pub playback_source: Option<LoadedWav>,
    pub wave_zoom: f32,
    pub wave_pan: f32,
}
