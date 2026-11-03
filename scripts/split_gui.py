"""One-off helper to split main.rs into app/, gui/, storage/."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "src"
lines = (ROOT / "main.rs").read_text(encoding="utf-8").splitlines(keepends=True)


def get(lo: int, hi: int) -> str:
    return "".join(lines[lo - 1 : hi])


(ROOT / "app").mkdir(exist_ok=True)
(ROOT / "gui").mkdir(exist_ok=True)

(ROOT / "app/types.rs").write_text(get(59, 146), encoding="utf-8")
(ROOT / "app/logic.rs").write_text(get(148, 896), encoding="utf-8")
(ROOT / "app/mod.rs").write_text(
    "mod logic;\nmod types;\n\npub use types::*;\n",
    encoding="utf-8",
)

storage = (
    "//! clips folder, settings file, and list ordering sidecars.\n\n"
    "use std::collections::{HashMap, HashSet};\n"
    "use std::path::{Path, PathBuf};\n"
    "use std::time::{Duration, SystemTime};\n\n"
    "use crate::app::ClipEntry;\n"
    "use crate::audio::AudioFormat;\n"
    "use crate::audio::clip_duration;\n"
    "use crate::hotkeys::{Command, HotkeyManager};\n\n"
    + get(1863, 2252)
)
for name in [
    "list_clips",
    "resolve_clips_dir",
    "ensure_clips_dir",
    "reveal_in_explorer",
    "open_folder_in_explorer",
    "settings_path",
    "save_order",
    "retarget_sidecar",
    "remove_from_sidecar",
    "arrange_clips",
    "list_visual_slot",
    "move_clip",
    "sanitize_stem",
    "same_clip_path",
    "new_clip_path",
    "load_settings",
    "write_settings",
    "format_recorded",
    "secs_to_time",
]:
    storage = storage.replace(f"fn {name}", f"pub(crate) fn {name}")
(ROOT / "storage.rs").write_text(storage, encoding="utf-8")

(ROOT / "gui/format.rs").write_text(
    get(2270, 2303)
    .replace("fn format_secs", "pub(crate) fn format_secs")
    .replace("fn format_clock", "pub(crate) fn format_clock")
    .replace("fn format_rate", "pub(crate) fn format_rate")
    .replace("fn channel_label", "pub(crate) fn channel_label")
    .replace("fn frame_secs", "pub(crate) fn frame_secs"),
    encoding="utf-8",
)

(ROOT / "gui/widgets.rs").write_text(
    "use eframe::egui;\n\n" + get(1845, 1861),
    encoding="utf-8",
)

gui_impl = (
    "use std::path::{Path, PathBuf};\n\n"
    "use eframe::egui;\n\n"
    "use crate::app::{HotkeyActions, SfxApp, WaveDrag};\n"
    "use crate::audio::{build_peaks, RegionEdit};\n"
    "use crate::gui::format::{channel_label, format_clock, format_rate, format_secs, frame_secs};\n"
    "use crate::hotkeys::Command;\n"
    "use crate::storage::{\n"
    "    list_visual_slot, move_clip, open_folder_in_explorer, reveal_in_explorer, save_order,\n"
    "};\n\n"
    "impl SfxApp {\n"
)

wave_extra = get(2254, 2268)

for name, lo, hi in [
    ("waveform.rs", 1628, 1842),
    ("clip_view.rs", 1482, 1626),
    ("clip_list.rs", 1240, 1480),
]:
    body = get(lo, hi)
    content = gui_impl + body
    if name == "waveform.rs":
        content += "\n" + wave_extra
    content += "}\n"
    (ROOT / "gui" / name).write_text(content, encoding="utf-8")

panels = (
    gui_impl
    + "use crate::gui::widgets::level_meter;\n\n"
    + get(1088, 1238)
    + "}\n"
)
(ROOT / "gui/panels.rs").write_text(panels, encoding="utf-8")

frame = (
    "use eframe::egui;\n\n"
    "use crate::app::{HotkeyActions, SfxApp};\n"
    "use crate::audio::RegionEdit;\n"
    "use crate::gui::widgets::level_meter;\n"
    "use crate::hotkeys::Command;\n\n"
    + get(897, 1086)
)
(ROOT / "gui/frame.rs").write_text(frame, encoding="utf-8")

(ROOT / "gui/mod.rs").write_text(
    "mod clip_list;\n"
    "mod clip_view;\n"
    "mod format;\n"
    "mod frame;\n"
    "mod panels;\n"
    "mod waveform;\n"
    "mod widgets;\n",
    encoding="utf-8",
)

logic = (ROOT / "app/logic.rs").read_text(encoding="utf-8")
logic = (
    "use std::collections::HashMap;\n"
    "use std::path::{Path, PathBuf};\n"
    "use std::time::{Duration, Instant, SystemTime};\n\n"
    "use eframe::egui;\n\n"
    "use crate::app::{\n"
    "    ClipEditHistory, ClipEntry, HotkeyActions, OpenClip, RenameState, SfxApp, WaveDrag,\n"
    "};\n"
    "use crate::audio::{\n"
    "    AudioFormat, InputChoice, LoadedWav, Playback, Recorder, RegionEdit, edit_samples,\n"
    "    read_clip, write_clip, write_recorded,\n"
    "};\n"
    "use crate::hotkeys::{Command, HotkeyManager};\n"
    "use crate::storage::{\n"
    "    list_clips, load_settings, new_clip_path, remove_from_sidecar, retarget_sidecar,\n"
    "    resolve_clips_dir, sanitize_stem, same_clip_path, save_order, settings_path,\n"
    "    write_settings,\n"
    "};\n\n"
    + logic
)
(ROOT / "app/logic.rs").write_text(logic, encoding="utf-8")

types = (ROOT / "app/types.rs").read_text(encoding="utf-8")
types = types.replace("struct ClipEntry", "pub(crate) struct ClipEntry")
types = types.replace("struct OpenClip", "pub(crate) struct OpenClip")
types = types.replace("struct RenameState", "pub(crate) struct RenameState")
types = types.replace("enum WaveDrag", "pub(crate) enum WaveDrag")
types = types.replace("struct ClipEditHistory", "pub(crate) struct ClipEditHistory")
types = types.replace("struct HotkeyActions", "pub(crate) struct HotkeyActions")
types = types.replace("struct SfxApp", "pub(crate) struct SfxApp")
(ROOT / "app/types.rs").write_text(types, encoding="utf-8")

main_new = get(1, 57).replace(
    "mod audio;\nmod hotkeys;",
    "mod app;\nmod audio;\nmod gui;\nmod hotkeys;\nmod storage;",
)
main_new += "\nuse app::SfxApp;\nuse storage::{move_clip, arrange_clips, sanitize_stem, secs_to_time};\nuse app::ClipEntry;\n\n"
main_new += get(2305, len(lines))
(ROOT / "main.rs").write_text(main_new, encoding="utf-8")

print("split complete")
