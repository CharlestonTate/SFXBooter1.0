// sfx record window.
// record from a picked mic, drop the clip in clips/, then trim or cut it.
// the list order is yours to drag. f2 renames the one you have open.
// hotkeys live in their own window and get saved with the mic and the format.

mod app;
mod audio;
mod gui;
mod hotkeys;
mod storage;

use app::SfxApp;
use eframe::egui;

fn app_icon() -> egui::IconData {
    let bytes = include_bytes!("../image.png");
    let decoder = png::Decoder::new(&bytes[..]);
    let mut reader = decoder.read_info().expect("logo png should decode");
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).expect("logo png frame");
    let width = info.width;
    let height = info.height;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        other => panic!("unsupported logo color type: {other:?}"),
    };
    egui::IconData {
        rgba,
        width,
        height,
    }
}

fn main() -> eframe::Result {
    let icon = app_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([760.0, 460.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "SFX Record",
        options,
        Box::new(|_cc| Ok(Box::new(SfxApp::new()))),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::app::ClipEntry;
    use crate::storage::{arrange_clips, move_clip, sanitize_stem, secs_to_time};

    use super::*;

    fn clip(name: &str, secs: u64) -> ClipEntry {
        ClipEntry {
            path: PathBuf::from(name),
            name: name.to_string(),
            duration: 1.0,
            recorded: secs_to_time(secs),
            recorded_label: String::new(),
        }
    }

    fn names(clips: &[ClipEntry]) -> Vec<&str> {
        clips.iter().map(|clip| clip.name.as_str()).collect()
    }

    #[test]
    fn dragging_down_and_up_lands_on_the_pointer_row() {
        let mut clips = vec![clip("a", 1), clip("b", 2), clip("c", 3)];
        move_clip(&mut clips, 0, 2);
        assert_eq!(names(&clips), ["b", "c", "a"]);
        move_clip(&mut clips, 2, 0);
        assert_eq!(names(&clips), ["a", "b", "c"]);
    }

    #[test]
    fn clips_missing_from_the_order_file_go_first() {
        let found = vec![clip("a", 10), clip("b", 20), clip("c", 30), clip("d", 5)];
        let arranged = arrange_clips(found, &["a".into(), "b".into()]);
        assert_eq!(names(&arranged), ["c", "d", "a", "b"]);
    }

    #[test]
    fn sanitize_stem_drops_a_typed_extension_and_bad_characters() {
        assert_eq!(sanitize_stem("Hello.WAV"), "Hello");
        assert_eq!(sanitize_stem("boom.flac"), "boom");
        assert_eq!(sanitize_stem("a/b:c"), "abc");
        assert_eq!(sanitize_stem("  .. "), "");
        assert_eq!(sanitize_stem("my.sound.aiff"), "my.sound");
    }
}
