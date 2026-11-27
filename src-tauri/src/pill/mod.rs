//! Corner Assistant Pill: floating graphite glass status pill with per-pixel alpha.

mod renderer;
mod win32;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub use renderer::TextBitmap;
pub use win32::{compute_pill_origin, spawn_pill_window};

#[derive(Clone, Debug, PartialEq)]
pub enum PillState {
    Hidden,
    Listening,
    Speech(String),
    Action(String, bool), // (command_text, success)
    Quit,
}

pub struct PillShared {
    pub state: PillState,
    pub dirty: bool,
}

pub struct CornerPill {
    pub shared: Arc<Mutex<PillShared>>,
}

impl CornerPill {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(PillShared {
                state: PillState::Hidden,
                dirty: false,
            })),
        }
    }

    pub fn show_listening(&self) {
        self.set(PillState::Listening);
        let shared = self.shared.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(4200));
            let mut s = shared.lock().unwrap();
            if matches!(s.state, PillState::Listening) {
                s.state = PillState::Hidden;
                s.dirty = true;
            }
        });
    }

    pub fn show_speech(&self, text: &str) {
        self.set(PillState::Speech(text.to_string()));
    }

    pub fn show_action(&self, cmd: &str, success: bool) {
        self.set(PillState::Action(cmd.to_string(), success));
        let shared = self.shared.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(2400));
            let mut s = shared.lock().unwrap();
            if matches!(s.state, PillState::Action(..)) {
                s.state = PillState::Hidden;
                s.dirty = true;
            }
        });
    }

    pub fn hide(&self) {
        self.set(PillState::Hidden);
    }

    pub fn destroy(&self) {
        self.set(PillState::Quit);
    }

    fn set(&self, new_state: PillState) {
        let mut s = self.shared.lock().unwrap();
        if s.state == PillState::Quit && new_state != PillState::Quit {
            return;
        }
        if s.state != new_state {
            s.state = new_state;
            s.dirty = true;
        }
    }
}

impl Default for CornerPill {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::renderer::{MARGIN, PH};
    use super::*;

    #[test]
    fn pill_stays_above_taskbar_even_when_workarea_equals_screen() {
        let full = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let (_, y) = compute_pill_origin(full, 1080, 1.0);
        let visible_bottom = y + ((PH + MARGIN).round() as i32);
        assert!(
            visible_bottom <= 1032 - 20,
            "pill visible bottom {visible_bottom} must clear 48px Win11 taskbar"
        );
    }
}
