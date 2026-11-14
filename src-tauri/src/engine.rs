use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::app_launcher::{self, App, scan_apps};
use crate::executor::execute_command;
use crate::input::MessageBeep;
use crate::island_bridge;
use crate::pill;
use crate::settings;
use crate::vocab::{load_aliases, load_vocab, Vocab};
use crate::voice;
use crate::wake_word::{ask, extract_command};
use crate::MIC_MUTED;

pub fn run_text_mode(
    vocab: &Vocab,

    mut apps: &mut Vec<App>,

    mut aliases: &mut HashMap<String, String>,
) {
    println!("\nType commands: open <app> [left/right/fullscreen] / close <app> / play <song> / volume 50% / lock / shutdown");

    println!("Commands: 'apps', 'windows', 'q' to quit.\n");

    loop {
        let line = ask("> ");

        let line = line.trim();

        if line.eq_ignore_ascii_case("q") {
            break;
        }

        execute_command(line, vocab, &mut apps, &mut aliases, None);
    }
}

pub fn start_voice_engine(settings_state: Arc<Mutex<settings::AuraSettings>>, app: AppHandle) {
    let vocab = load_vocab();
    let mut aliases = load_aliases();
    let mut apps = scan_apps();
    let require_wake_word = true;

    loop {
        // When Voice is turned OFF (or inactive during --silent PC boot), keep zero mic handles,
        // zero CornerPill windows/threads, and minimal idle memory.
        while !settings_state.lock().unwrap().module_should_run_now("voice") {
            voice::set_listening_allowed(false);
            std::thread::sleep(Duration::from_millis(300));
        }
        voice::set_listening_allowed(true);

        let pill = std::sync::Arc::new(pill::CornerPill::new());
        pill::spawn_pill_window(pill.shared.clone());

        let mut v = match voice::Voice::new() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to initialize voice: {e}");
                pill.destroy();
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        v.calibrate();

        loop {
            if !settings_state.lock().unwrap().module_should_run_now("voice") {
                break;
            }

            if MIC_MUTED.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            match v.wait_for_wake_word() {
                Ok(Some(heard)) => {
                    let raw = heard.text.trim();

                    if raw.is_empty() {
                        continue;
                    }

                    let (current_wake, pill_enabled, island_enabled) = {
                        let s = settings_state.lock().unwrap();

                        (s.wake_word.clone(), s.corner_pill, s.show_on_island)
                    };

                    let show_pill = pill_enabled && !island_enabled;

                    let matched_cmd = std::iter::once(raw)
                        .chain(heard.alternatives.iter().map(|s| s.trim()))
                        .filter(|s| !s.is_empty())
                        .find_map(|cand| extract_command(cand, require_wake_word, &current_wake));

                    if let Some(cmd) = matched_cmd {
                        if cmd.is_empty() {
                            unsafe {
                                MessageBeep(0x00000040);
                            }

                            if island_enabled {
                                island_bridge::notify_wake();
                            } else if show_pill {
                                pill.show_listening();
                            } else {
                                pill.hide();
                            }

                            let _ = app.emit(
                                "voice-event",
                                serde_json::json!({
                                    "type": "wake",
                                    "text": "Listening for your command...",
                                    "status": "Listening"
                                }),
                            );

                            v.reset_buffers();

                            if let Ok(Some(next_heard)) = v.wait_for_command() {
                                let next_cmd = next_heard.text.trim();

                                if !next_cmd.is_empty() {
                                    if island_enabled {
                                        island_bridge::notify_thinking(Some(next_cmd));
                                    } else if show_pill {
                                        pill.show_speech(next_cmd);
                                    }

                                    let _ = app.emit(
                                        "voice-event",
                                        serde_json::json!({
                                            "type": "speech",
                                            "text": format!("Heard: \"{}\"", next_cmd),
                                            "status": "Processing"
                                        }),
                                    );

                                    std::thread::sleep(Duration::from_millis(500));

                                    let res = execute_command(
                                        next_cmd,
                                        &vocab,
                                        &mut apps,
                                        &mut aliases,
                                        Some(&v),
                                    );

                                    if island_enabled {
                                        island_bridge::notify_done(res);
                                    } else if show_pill {
                                        pill.show_action(next_cmd, res);
                                    }

                                    let _ = app.emit("voice-event", serde_json::json!({
                                        "type": "action",
                                        "text": format!("Action: \"{}\" ({})", next_cmd, if res { "Success" } else { "Failed" }),
                                        "status": if res { "Completed" } else { "Unknown" }
                                    }));
                                } else {
                                    if island_enabled { island_bridge::notify_cancel(); }
                                    else if show_pill { pill.hide(); }
                                }
                            } else {
                                if island_enabled { island_bridge::notify_cancel(); }
                                else if show_pill { pill.hide(); }
                            }
                        } else {
                            unsafe {
                                MessageBeep(0x00000040);
                            }

                            if island_enabled {
                                island_bridge::notify_thinking(Some(&cmd));
                            } else if show_pill {
                                pill.show_speech(&cmd);
                            } else {
                                pill.hide();
                            }

                            let _ = app.emit(
                                "voice-event",
                                serde_json::json!({
                                    "type": "speech",
                                    "text": format!("Heard: \"{}\"", cmd),
                                    "status": "Processing"
                                }),
                            );

                            std::thread::sleep(Duration::from_millis(500));

                            let res = execute_command(&cmd, &vocab, &mut apps, &mut aliases, Some(&v));

                            if island_enabled {
                                island_bridge::notify_done(res);
                            } else if show_pill {
                                pill.show_action(&cmd, res);
                            }

                            let _ = app.emit("voice-event", serde_json::json!({
                                "type": "action",
                                "text": format!("Action: \"{}\" ({})", cmd, if res { "Success" } else { "Failed" }),
                                "status": if res { "Completed" } else { "Unknown" }
                            }));
                        }
                    }
                }

                Ok(None) => {}

                Err(e) => {
                    eprintln!("Voice error: {e}");
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }

        // Shutdown Voice resources completely when turned OFF: close mic stream + destroy pill HWND + trim RAM.
        voice::set_listening_allowed(false);
        pill.destroy();
        drop(v);
        unsafe {
            let _ = windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
            );
        }
    }
}