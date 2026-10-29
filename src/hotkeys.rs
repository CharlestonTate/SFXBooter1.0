// hotkey manager. every command has one key, saved with the rest of the settings.
// the window is where you rebind them. press set, then the key you want.
// escape backs out of a rebind and does not get stored.

use eframe::egui;

// the things a key can fire. order here is the order in the manager window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ToggleRecord,
    Play,
    PlaySelection,
    Stop,
    Trim,
    Cut,
    Rename,
    ShowHotkeys,
}

impl Command {
    pub const ALL: [Command; 8] = [
        Command::ToggleRecord,
        Command::Play,
        Command::PlaySelection,
        Command::Stop,
        Command::Trim,
        Command::Cut,
        Command::Rename,
        Command::ShowHotkeys,
    ];

    // short id writen into the settings file
    fn id(self) -> &'static str {
        match self {
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

    pub fn title(self) -> &'static str {
        match self {
            Command::ToggleRecord => "Record / stop",
            Command::Play => "Play",
            Command::PlaySelection => "Play selection",
            Command::Stop => "Stop playback",
            Command::Trim => "Trim",
            Command::Cut => "Cut",
            Command::Rename => "Rename open clip",
            Command::ShowHotkeys => "Open this window",
        }
    }

    fn from_id(id: &str) -> Option<Command> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
}

pub struct HotkeyManager {
    // parallel to Command::ALL so a missing line just means unbound
    keys: [Option<egui::Key>; 8],
    // wich command is waiting for the next keypress
    waiting: Option<Command>,
    pub open: bool,
    pub dirty: bool,
}

impl HotkeyManager {
    pub fn defaults() -> Self {
        let mut manager = Self {
            keys: [None; 8],
            waiting: None,
            open: false,
            dirty: false,
        };
        // these are the keys you get the first time the app runs
        manager.set(Command::ToggleRecord, Some(egui::Key::R));
        manager.set(Command::Play, Some(egui::Key::Space));
        manager.set(Command::PlaySelection, Some(egui::Key::P));
        manager.set(Command::Stop, Some(egui::Key::S));
        manager.set(Command::Trim, Some(egui::Key::T));
        manager.set(Command::Cut, Some(egui::Key::C));
        manager.set(Command::Rename, Some(egui::Key::F2));
        manager.set(Command::ShowHotkeys, Some(egui::Key::F1));
        manager.dirty = false;
        manager
    }

    pub fn label(&self, command: Command) -> String {
        match self.keys[command as usize] {
            Some(key) => key.symbol_or_name().to_string(),
            None => "unbound".to_string(),
        }
    }

    // true while the manager is waiting to recieve a new key
    #[allow(dead_code)]
    pub fn is_capturing(&self) -> bool {
        self.waiting.is_some()
    }

    // the token writen next to bind. in the settings file
    pub fn saved_value(&self, command: Command) -> String {
        self.keys[command as usize]
            .map(|key| key.name().to_string())
            .unwrap_or_else(|| "unbound".to_string())
    }

    // true when this command's key went down this frame.
    // the key is consumed so a focused button does not also click.
    pub fn take(&self, ctx: &egui::Context, command: Command) -> bool {
        let Some(key) = self.keys[command as usize] else {
            return false;
        };
        ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key))
    }

    // while a rebind is armed, the next key becomes that command.
    // returns true if the key should not also run a command.
    pub fn poll_capture(&mut self, ctx: &egui::Context) -> bool {
        let Some(command) = self.waiting else {
            return false;
        };
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.waiting = None;
            return true;
        }
        let pressed = ctx.input(|input| {
            egui::Key::ALL.iter().copied().find(|key| {
                *key != egui::Key::Escape && !is_modifier(*key) && input.key_pressed(*key)
            })
        });
        if let Some(key) = pressed {
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
            self.assign(command, key);
            self.waiting = None;
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub fn binding_line(&self, command: Command) -> Option<String> {
        self.keys[command as usize].map(|key| format!("bind.{}={}", command.id(), key.name()))
    }

    pub fn apply_binding(&mut self, id: &str, key_name: &str) {
        let Some(command) = Command::from_id(id) else {
            return;
        };
        if key_name.is_empty() || key_name == "unbound" {
            self.set(command, None);
            return;
        }
        if let Some(key) = egui::Key::from_name(key_name) {
            self.assign(command, key);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Click Set, then press the key you want. Escape cancels.");
        ui.add_space(6.0);
        for command in Command::ALL {
            ui.horizontal(|ui| {
                ui.label(command.title());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear").clicked() {
                        self.set(command, None);
                        if self.waiting == Some(command) {
                            self.waiting = None;
                        }
                    }
                    if ui.button("Set").clicked() {
                        // arm this row. the actual key is read next frame in poll_capture
                        self.waiting = Some(command);
                    }
                    let text = if self.waiting == Some(command) {
                        "press a key".to_string()
                    } else {
                        self.label(command)
                    };
                    ui.monospace(text);
                });
            });
        }
    }

    fn assign(&mut self, command: Command, key: egui::Key) {
        // one key, one command. the old owner looses it.
        for slot in &mut self.keys {
            if *slot == Some(key) {
                *slot = None;
            }
        }
        self.set(command, Some(key));
    }

    fn set(&mut self, command: Command, key: Option<egui::Key>) {
        self.keys[command as usize] = key;
        self.dirty = true;
    }
}

// shift and friends are not real hotkeys on thier own
fn is_modifier(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}
