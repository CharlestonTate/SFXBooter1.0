//! clips folder, settings file, and list ordering sidecars.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::app::ClipEntry;
use crate::audio::AudioFormat;
use crate::audio::clip_duration;
use crate::hotkeys::{Command, HotkeyManager};

fn default_clips_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("clips")
}

pub(crate) fn resolve_clips_dir(saved: Option<&str>) -> (PathBuf, Option<String>, String) {
    let chosen = saved
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_clips_dir);
    match ensure_clips_dir(&chosen) {
        Ok(dir) => (dir.clone(), None, dir.display().to_string()),
        Err(err) => {
            let fallback = default_clips_dir();
            let _ = ensure_clips_dir(&fallback);
            (fallback, Some(err), chosen.display().to_string())
        }
    }
}

pub(crate) fn ensure_clips_dir(path: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(path)
        .map_err(|err| format!("Could not use the clips folder ({}): {err}", path.display()))?;
    Ok(path.to_path_buf())
}

pub(crate) fn reveal_in_explorer(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let path = path.canonicalize().map_err(|err| err.to_string())?;
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Show in Explorer is only supported on Windows.".into())
    }
}

pub(crate) fn open_folder_in_explorer(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let path = path.canonicalize().map_err(|err| err.to_string())?;
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Open folder is only supported on Windows.".into())
    }
}

pub(crate) fn settings_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("sfxrecord.settings")
}

// the created time on the file can change when an edit occured,
// so the recorded date is kept seperate in clips/.dates
pub(crate) fn list_clips(dir: &Path) -> Vec<ClipEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dates = load_dates(dir);
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for item in read.flatten() {
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if AudioFormat::from_path(&path).is_none() {
            continue;
        }
        let name = name.to_string();
        let recorded = if let Some(time) = dates.get(&name).copied() {
            time
        } else {
            let time = item
                .metadata()
                .ok()
                .map(file_recorded_time)
                .unwrap_or_else(SystemTime::now);
            dates.insert(name.clone(), time);
            time
        };
        seen.insert(name.clone());
        let duration = clip_duration(&path).unwrap_or(0.0);
        found.push(ClipEntry {
            path,
            name,
            duration,
            recorded,
            recorded_label: format_recorded(recorded),
        });
    }
    dates.retain(|name, _| seen.contains(name));
    save_dates(dir, &dates);
    let order = load_order(dir);
    // new clips that are not in the order file yet go to the top, newest first,
    // becuase that is where you just recorded
    let clips = arrange_clips(found, &order);
    save_order(dir, &clips);
    clips
}

fn file_recorded_time(meta: std::fs::Metadata) -> SystemTime {
    meta.created()
        .or_else(|_| meta.modified())
        .unwrap_or_else(|_| SystemTime::now())
}

fn load_order(dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join(".order")).unwrap_or_default();
    text.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn save_order(dir: &Path, clips: &[ClipEntry]) {
    let body = clips
        .iter()
        .map(|clip| clip.name.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(dir.join(".order"), body);
}

fn load_dates(dir: &Path) -> HashMap<String, SystemTime> {
    let text = std::fs::read_to_string(dir.join(".dates")).unwrap_or_default();
    let mut dates = HashMap::new();
    for line in text.lines() {
        let Some((name, secs)) = line.split_once('\t') else {
            continue;
        };
        let Ok(secs) = secs.trim().parse::<u64>() else {
            continue;
        };
        dates.insert(name.to_string(), secs_to_time(secs));
    }
    dates
}

fn save_dates(dir: &Path, dates: &HashMap<String, SystemTime>) {
    let mut lines = dates
        .iter()
        .map(|(name, time)| format!("{name}\t{}", time_to_secs(*time)))
        .collect::<Vec<_>>();
    lines.sort();
    let _ = std::fs::write(dir.join(".dates"), lines.join("\n"));
}

pub(crate) fn retarget_sidecar(dir: &Path, old: &str, new_name: &str) {
    let mut dates = load_dates(dir);
    if let Some(time) = dates.remove(old) {
        dates.insert(new_name.to_string(), time);
    }
    save_dates(dir, &dates);
    let mut order = load_order(dir);
    for name in &mut order {
        if name == old {
            *name = new_name.to_string();
        }
    }
    let _ = std::fs::write(dir.join(".order"), order.join("\n"));
}

pub(crate) fn remove_from_sidecar(dir: &Path, name: &str) {
    let mut dates = load_dates(dir);
    dates.remove(name);
    save_dates(dir, &dates);
    let order = load_order(dir)
        .into_iter()
        .filter(|entry| entry != name)
        .collect::<Vec<_>>();
    let _ = std::fs::write(dir.join(".order"), order.join("\n"));
}

pub(crate) fn arrange_clips(found: Vec<ClipEntry>, order: &[String]) -> Vec<ClipEntry> {
    let mut leftover = found
        .into_iter()
        .map(|clip| (clip.name.clone(), clip))
        .collect::<HashMap<_, _>>();
    let mut ordered = Vec::new();
    for name in order {
        if let Some(clip) = leftover.remove(name) {
            ordered.push(clip);
        }
    }
    let mut fresh = leftover.into_values().collect::<Vec<_>>();
    fresh.sort_by(|a, b| {
        b.recorded
            .cmp(&a.recorded)
            .then_with(|| a.name.cmp(&b.name))
    });
    fresh.append(&mut ordered);
    fresh
}

// while dragging, where each row should sit before the drop commits
pub(crate) fn list_visual_slot(
    index: usize,
    drag_from: Option<usize>,
    drag_to: Option<usize>,
) -> usize {
    let (from, to) = match (drag_from, drag_to) {
        (Some(from), Some(to)) if from != to => (from, to),
        _ => return index,
    };
    if from < to {
        if index == from {
            to
        } else if index > from && index <= to {
            index - 1
        } else {
            index
        }
    } else if index == from {
        to
    } else if index >= to && index < from {
        index + 1
    } else {
        index
    }
}

// `to` is the row the pointer is on. the clip ends up in that slot.
pub(crate) fn move_clip(clips: &mut [ClipEntry], from: usize, to: usize) {
    if from >= clips.len() || from == to {
        return;
    }
    let to = to.min(clips.len() - 1);
    if from == to {
        return;
    }
    // slice rotate keeps the other rows in their relative order
    if from < to {
        clips[from..=to].rotate_left(1);
    } else {
        clips[to..=from].rotate_right(1);
    }
}

fn time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn secs_to_time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

pub(crate) fn format_recorded(time: SystemTime) -> String {
    let datetime = chrono::DateTime::<chrono::Local>::from(time);
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

// strip a typed extension and characters windows will not put in a file name.
// the cut uses the lowercased lenght so the stem keeps its own caps.
pub(crate) fn sanitize_stem(raw: &str) -> String {
    let mut stem = raw.trim().to_string();
    let lower = stem.to_ascii_lowercase();
    for extension in [".wav", ".aiff", ".aif", ".flac", ".mp3"] {
        if let Some(stripped) = lower.strip_suffix(extension) {
            stem.truncate(stripped.len());
            break;
        }
    }
    let cleaned = stem
        .chars()
        .filter(|ch| {
            !matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !ch.is_control()
        })
        .collect::<String>();
    cleaned.trim().trim_matches('.').trim().to_string()
}

pub(crate) fn same_clip_path(a: &Path, b: &Path) -> bool {
    a == b
        || a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
}

pub(crate) fn new_clip_path(dir: &Path, format: AudioFormat) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let extension = format.extension();
    let mut path = dir.join(format!("sfx-{stamp}.{extension}"));
    let mut suffix = 2u32;
    while path.exists() {
        path = dir.join(format!("sfx-{stamp}-{suffix}.{extension}"));
        suffix += 1;
    }
    path
}

pub(crate) fn load_settings(
    path: &Path,
    hotkeys: &mut HotkeyManager,
) -> (
    Option<String>,
    Option<String>,
    Option<AudioFormat>,
    Option<String>,
) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut mic_id = None;
    let mut mic_name = None;
    let mut format = None;
    let mut clips_dir = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "mic" => {
                if !value.is_empty() {
                    mic_id = Some(value.to_string());
                }
            }
            "mic_name" => mic_name = Some(value.to_string()),
            "format" => format = AudioFormat::parse(value),
            "clips_dir" => {
                if !value.is_empty() {
                    clips_dir = Some(value.to_string());
                }
            }
            _ => {
                if let Some(id) = key.strip_prefix("bind.") {
                    hotkeys.apply_binding(id, value);
                }
            }
        }
    }
    hotkeys.dirty = false;
    (mic_id, mic_name, format, clips_dir)
}

pub(crate) fn write_settings(
    path: &Path,
    mic_id: Option<&str>,
    mic_name: &str,
    format: AudioFormat,
    clips_dir: &Path,
    hotkeys: &HotkeyManager,
) -> Result<(), String> {
    let mut lines = Vec::new();
    if let Some(id) = mic_id {
        lines.push(format!("mic={id}"));
    }
    lines.push(format!("mic_name={mic_name}"));
    lines.push(format!("format={}", format.as_str()));
    lines.push(format!("clips_dir={}", clips_dir.display()));
    for command in Command::ALL {
        lines.push(format!(
            "bind.{}={}",
            command_id(command),
            hotkeys.saved_value(command)
        ));
    }
    std::fs::write(path, lines.join("\n")).map_err(|err| err.to_string())
}

fn command_id(command: Command) -> &'static str {
    match command {
        Command::ToggleRecord => "toggle_record",
        Command::Play => "play",
        Command::PlaySelection => "play_selection",
        Command::Stop => "stop",
        Command::Trim => "trim",
        Command::Cut => "cut",
        Command::Rename => "rename",
        Command::ShowHotkeys => "hotkeys",
    }
}
