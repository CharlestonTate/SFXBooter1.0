# SFXBooter1.0
Dead simple SFX and audio engine written completely in rust. Record and edit WAV, AIFF, FLAC, and MP3 clips.

## Source layout
- `src/main.rs` — entry point
- `src/app/` — app state, recording, playback, edits
- `src/gui/` — egui panels, waveform, clip list
- `src/storage.rs` — clips folder, settings, list order sidecars
- `src/audio.rs` — codecs and I/O
- `src/hotkeys.rs` — key bindings
