use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::ai;
use crate::app_launcher::scan_apps;
use crate::executor::execute_command;
use crate::island_bridge;
use crate::pill;
use crate::settings;
use crate::sound;
use crate::vocab::{load_aliases, load_vocab};
use crate::voice;
use crate::wake_word::extract_command;
use crate::MIC_MUTED;

#[allow(clippy::too_many_arguments)]
fn dispatch_heard_command(
    raw_cmd: &str,
    settings_state: &Arc<Mutex<settings::AuraSettings>>,
    vocab: &crate::vocab::Vocab,
    apps: &mut [crate::app_launcher::App],
    aliases: &mut std::collections::HashMap<String, String>,
    v: &voice::Voice,
    pill: &pill::CornerPill,
    show_pill: bool,
    island_enabled: bool,
    app: &AppHandle,
) {
    let ai_cfg = settings_state.lock().unwrap().ai.clone();

    if let Some(ok) = ai::try_voice_dictation(&ai_cfg, raw_cmd, pill, show_pill, app) {
        if island_enabled {
            island_bridge::notify_done(ok);
        }
        return;
    }

    let res = execute_command(raw_cmd, vocab, apps, aliases, Some(v));
    if res {
        if island_enabled {
            island_bridge::notify_done(true);
        } else if show_pill {
            pill.show_action(raw_cmd, true);
        }
        let _ = app.emit(
            "voice-event",
            serde_json::json!({
                "type": "action",
                "text": format!("Action: \"{}\" (Success)", raw_cmd),
                "status": "Completed"
            }),
        );
        ai::maybe_speak_fast_ack(&ai_cfg, raw_cmd);
        return;
    }

    if ai_cfg.enabled {
        let ai_ok = ai::handle_voice_fallback(
            &ai_cfg, raw_cmd, vocab, apps, aliases, Some(v), pill, show_pill, island_enabled, app,
        );
        if island_enabled {
            island_bridge::notify_done(ai_ok);
        }
        return;
    }

    if island_enabled {
        island_bridge::notify_done(false);
    } else if show_pill {
        pill.show_action(raw_cmd, false);
    }
    let _ = app.emit(
        "voice-event",
        serde_json::json!({
            "type": "action",
            "text": format!("Action: \"{}\" (Failed)", raw_cmd),
            "status": "Unknown"
        }),
    );
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
                            sound::play_wake_chime();

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

                            let next_heard_res = if island_enabled {
                                v.wait_for_command_with_level(island_bridge::notify_level)
                            } else {
                                v.wait_for_command()
                            };

                            if let Ok(Some(next_heard)) = next_heard_res {
                                let next_cmd = next_heard.text.trim();

                                if !next_cmd.is_empty() {
                                    if island_enabled {
                                        island_bridge::notify_thinking(None);
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

                                    dispatch_heard_command(
                                        next_cmd,
                                        &settings_state,
                                        &vocab,
                                        &mut apps,
                                        &mut aliases,
                                        &v,
                                        &pill,
                                        show_pill,
                                        island_enabled,
                                        &app,
                                    );
                                } else {
                                    if island_enabled { island_bridge::notify_cancel(); }
                                    else if show_pill { pill.hide(); }
                                }
                            } else {
                                if island_enabled { island_bridge::notify_cancel(); }
                                else if show_pill { pill.hide(); }
                            }
                        } else {
                            sound::play_wake_chime();

                            if island_enabled {
                                island_bridge::notify_thinking(None);
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

                            dispatch_heard_command(
                                &cmd,
                                &settings_state,
                                &vocab,
                                &mut apps,
                                &mut aliases,
                                &v,
                                &pill,
                                show_pill,
                                island_enabled,
                                &app,
                            );
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