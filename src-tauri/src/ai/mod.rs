//! Standalone Voice AI Assistant & Dictation module (PC-only; zero Android/phone AI sync).
//! Fast local voice commands (`executor/`) always run first; AI activates only for fallback,
//! error explanations, casual conversation, screen OCR questions, or smart dictation.

pub mod commands;
pub mod dictation;
pub mod local_llm;
pub mod providers;
pub mod screen_context;
pub mod terminal_guard;
pub mod tts;
pub mod vault;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

use crate::app_launcher::App;
use crate::executor::execute_command;
use crate::pill::CornerPill;
use crate::vocab::Vocab;
use crate::voice::Voice;

use providers::ChatTurn;

fn default_true() -> bool {
    true
}

fn default_routing_mode() -> String {
    "hybrid".to_string()
}

fn default_local_model() -> String {
    "llama3.2:3b".to_string()
}

fn default_voice_style() -> String {
    "google_natural".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub voice_reply: bool,
    #[serde(default = "default_voice_style")]
    pub voice_style: String, // "google_natural" | "male_george" | "male_ravi"
    #[serde(default = "default_true")]
    pub dictation_enabled: bool,
    #[serde(default = "default_true")]
    pub screen_context: bool,
    #[serde(default)]
    pub local_llm_enabled: bool,
    #[serde(default = "default_local_model")]
    pub local_model: String,
    #[serde(default = "default_routing_mode")]
    pub routing_mode: String, // "hybrid" | "api_only" | "local_first" | "local_only"
    #[serde(default)]
    pub allow_ai_terminal: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            voice_reply: true,
            voice_style: default_voice_style(),
            dictation_enabled: true,
            screen_context: true,
            local_llm_enabled: false,
            local_model: default_local_model(),
            routing_mode: default_routing_mode(),
            allow_ai_terminal: false,
        }
    }
}

impl AiConfig {
    pub fn normalize(&mut self) {
        if self.local_model.trim().is_empty() {
            self.local_model = default_local_model();
        }
        if !matches!(
            self.voice_style.as_str(),
            "google_natural" | "male_george" | "male_ravi"
        ) {
            self.voice_style = default_voice_style();
        }
        if !matches!(
            self.routing_mode.as_str(),
            "hybrid" | "api_only" | "local_first" | "local_only"
        ) {
            self.routing_mode = default_routing_mode();
        }
    }
}

fn history_lock() -> &'static Mutex<VecDeque<ChatTurn>> {
    static HIST: OnceLock<Mutex<VecDeque<ChatTurn>>> = OnceLock::new();
    HIST.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub fn clear_conversation() {
    let mut h = history_lock().lock().unwrap_or_else(|e| e.into_inner());
    h.clear();
}

fn push_turn(role: &'static str, text: String) {
    let mut h = history_lock().lock().unwrap_or_else(|e| e.into_inner());
    while h.len() >= 10 {
        h.pop_front();
    }
    h.push_back(ChatTurn { role, text });
}

fn snapshot_history_plus(user_msg: String) -> Vec<ChatTurn> {
    let h = history_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<ChatTurn> = h.iter().cloned().collect();
    out.push(ChatTurn {
        role: "user",
        text: user_msg,
    });
    out
}

fn extract_json_object(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end > start {
        serde_json::from_str::<Value>(&trimmed[start..=end]).ok()
    } else {
        None
    }
}

fn build_system_prompt(
    cfg: &AiConfig,
    win_title: &str,
    open_apps: &[String],
    screen_ctx: Option<&screen_context::ScreenSnapshot>,
    failed_fast_cmd: bool,
) -> String {
    let mut p = String::from(
        "You are Nexus AI, a crisp, witty, Siri/Google-style voice assistant on the user's Windows PC. \
         Reply in the same language the user used (if Hindi, write clean Devanagari Hindi script so speech sounds natural; if Bengali, write Bengali script; if English, clear concise English). \
         Keep replies ultra-short (1-2 short spoken sentences, under 30 words) with zero markdown or lists.\n\
         Respond ONLY with a single JSON object matching:\n\
         {\"reply\": \"your short natural answer\", \"pc_command\": null, \"terminal_command\": null, \"terminal_reason\": null}\n\
         Rules:\n\
         - If the user casually asks to open or close an app (Task Manager style kill via \"close chrome\" / \"close spotify\"), control Spotify/YouTube/volume/brightness, take a screenshot, or set a timer, set \"pc_command\" to a short direct command like \"open chrome\", \"close spotify\", \"play spotify\", \"volume down\", \"mute\", or \"pause\". Otherwise keep it null.\n\
         - NEVER attempt messaging apps (WhatsApp/Telegram/Discord) or file copy/move/delete.\n"
    );

    if cfg.allow_ai_terminal {
        p.push_str(
            "- User enabled opt-in AI Terminal: for read-only system checks (e.g. `Get-Process | Sort-Object WorkingSet64 -Desc | Select -First 5`, `Get-Volume`, `Get-NetIPAddress`) you may set \"terminal_command\" and Nexus will inspect it immediately; mutating commands wait for UI confirmation, and destructive commands (delete/format/reg delete) are forbidden.\n",
        );
    } else {
        p.push_str("- \"terminal_command\" MUST ALWAYS be null (terminal disabled).\n");
    }

    if failed_fast_cmd {
        p.push_str("- Note: Nexus's built-in fast command parser already tried this phrase and could not find a direct match; explain briefly or respond conversationally.\n");
    }

    if !win_title.trim().is_empty() {
        p.push_str(&format!("\nUser's Active Window Title: \"{}\"\n", win_title.trim()));
    }
    if !open_apps.is_empty() {
        p.push_str(&format!("Open Desktop/Taskbar Windows Right Now: {}\n", open_apps.join(" | ")));
    }

    if let Some(sc) = screen_ctx {
        p.push_str(&format!(
            "REAL-TIME SCREEN OCR PROVIDED BELOW — you CAN see what is on the user's PC screen right now. Answer directly from the open windows & OCR text below and never claim you cannot see the screen:\n```\n{}\n```\n",
            if sc.ocr_text.is_empty() {
                "(Screen had no readable text)"
            } else {
                &sc.ocr_text
            }
        ));
    }
    p
}

pub struct AiOutcome {
    pub reply: String,
    pub executed_cmd: Option<String>,
    pub proposal: Option<terminal_guard::PendingTerminalProposal>,
    pub meta: String,
}

fn speak_with_island_sync(text: &str, voice_style: &str, island_enabled: bool) {
    tts::speak_blocking(
        text,
        voice_style,
        || {
            if island_enabled {
                crate::island_bridge::notify_speaking();
            }
        },
        |lv| {
            if island_enabled {
                crate::island_bridge::notify_level(lv);
            }
        },
        || {
            if island_enabled {
                crate::island_bridge::notify_speaking();
            }
        },
    );
}

/// Core conversational / fallback handler (used both from voice fallback and Voice UI chat tester).
#[allow(clippy::too_many_arguments)]
pub fn ask_ai(
    cfg: &AiConfig,
    user_text: &str,
    failed_fast_cmd: bool,
    vocab: &Vocab,
    apps: &mut [App],
    aliases: &mut HashMap<String, String>,
    voice_inst: Option<&Voice>,
    island_enabled: bool,
    speak_sync: bool,
    app: &AppHandle,
) -> Result<AiOutcome, String> {
    let sc = if cfg.screen_context && screen_context::looks_like_screen_query(user_text) {
        screen_context::capture_screen_context().ok()
    } else {
        None
    };
    let win_title = sc
        .as_ref()
        .map(|s| s.active_window.clone())
        .unwrap_or_else(screen_context::active_window_title);
    let open_apps = if sc.is_some() {
        screen_context::open_window_apps()
    } else {
        Vec::new()
    };

    let sys = build_system_prompt(cfg, &win_title, &open_apps, sc.as_ref(), failed_fast_cmd);
    let turns = snapshot_history_plus(user_text.to_string());
    let raw_reply = providers::complete(
        &cfg.routing_mode,
        cfg.local_llm_enabled,
        &cfg.local_model,
        &sys,
        &turns,
    )?;

    let mut reply_text = raw_reply.text.clone();
    let mut executed_cmd = None;
    let mut proposal = None;

    if let Some(v) = extract_json_object(&raw_reply.text) {
        if let Some(r) = v.get("reply").and_then(|x| x.as_str()) {
            if !r.trim().is_empty() {
                reply_text = r.trim().to_string();
            }
        }
        if let Some(pc_cmd) = v.get("pc_command").and_then(|x| x.as_str()) {
            let clean = pc_cmd.trim();
            if !clean.is_empty()
                && dictation::parse_dictation_intent(clean).is_none()
                && execute_command(clean, vocab, apps, aliases, voice_inst)
            {
                executed_cmd = Some(clean.to_string());
            }
        }
        if cfg.allow_ai_terminal {
            if let Some(t_cmd) = v.get("terminal_command").and_then(|x| x.as_str()) {
                let clean = t_cmd.trim();
                if !clean.is_empty() {
                    if terminal_guard::is_safe_readonly_query(clean) {
                        if let Ok(diag_out) = terminal_guard::run_readonly_checked(clean) {
                            let brief: String = diag_out.lines().take(4).collect::<Vec<_>>().join(", ");
                            if !brief.is_empty() && !reply_text.contains(&brief) {
                                reply_text = format!("{} ({})", reply_text, brief);
                            }
                        }
                    } else {
                        let reason = v
                            .get("terminal_reason")
                            .and_then(|x| x.as_str())
                            .unwrap_or("Suggested PowerShell command")
                            .to_string();
                        if let Ok(prop) = terminal_guard::enqueue_proposal(clean.to_string(), reason) {
                            proposal = Some(prop.clone());
                            let _ = app.emit("ai-terminal-proposal", &prop);
                        }
                    }
                }
            }
        }
    }

    push_turn("user", user_text.to_string());
    push_turn("assistant", reply_text.clone());

    let meta = format!("{} · {}", raw_reply.used_label, raw_reply.used_model);
    let _ = app.emit(
        "ai-reply",
        json!({
            "user": user_text,
            "reply": reply_text,
            "meta": meta,
            "executed_cmd": executed_cmd,
            "proposal": proposal
        }),
    );

    if cfg.voice_reply {
        if speak_sync {
            speak_with_island_sync(&reply_text, &cfg.voice_style, island_enabled);
        } else {
            let txt = reply_text.clone();
            let style = cfg.voice_style.clone();
            std::thread::spawn(move || {
                speak_with_island_sync(&txt, &style, island_enabled);
                if island_enabled {
                    crate::island_bridge::notify_done(true);
                }
            });
        }
    }

    Ok(AiOutcome {
        reply: reply_text,
        executed_cmd,
        proposal,
        meta,
    })
}

/// Handles fast voice dictation ("type ..." / "smart type ...") before command matching.
/// Returns `Some(true/false)` if the phrase was a dictation request, or `None` for normal commands.
pub fn try_voice_dictation(cfg: &AiConfig, raw_cmd: &str, pill: &CornerPill, show_pill: bool, app: &AppHandle) -> Option<bool> {
    if !cfg.dictation_enabled {
        return None;
    }
    let intent = dictation::parse_dictation_intent(raw_cmd)?;
    let to_type = match intent {
        dictation::DictationIntent::Raw(txt) => txt,
        dictation::DictationIntent::Smart(txt) => {
            if cfg.enabled {
                let prompt = vec![ChatTurn {
                    role: "user",
                    text: txt.clone(),
                }];
                providers::complete(
                    &cfg.routing_mode,
                    cfg.local_llm_enabled,
                    &cfg.local_model,
                    "Fix punctuation, spelling, and formatting cleanly in the same language. Reply ONLY with the final text to type, zero quotes or commentary.",
                    &prompt,
                )
                .map(|r| r.text)
                .unwrap_or(txt)
            } else {
                txt
            }
        }
    };

    let ok = dictation::type_unicode_at_cursor(&to_type).is_ok();
    if show_pill {
        pill.show_action(&format!("Typed: {}", to_type), ok);
    }
    let _ = app.emit(
        "voice-event",
        json!({
            "type": "action",
            "text": format!("Typed: \"{}\"", to_type),
            "status": if ok { "Dictated" } else { "Failed" }
        }),
    );
    Some(ok)
}

/// Optional instant spoken confirmation when AI is enabled and fast local executor succeeded (0ms LLM cost).
pub fn maybe_speak_fast_ack(cfg: &AiConfig, cmd: &str) {
    if cfg.enabled && cfg.voice_reply {
        let phrase = format!("Done: {}", cmd);
        let style = cfg.voice_style.clone();
        std::thread::spawn(move || {
            tts::speak_blocking(&phrase, &style, || {}, |_| {}, || {});
        });
    }
}

/// Fallback invoked only when local `executor` could not execute `cmd`.
#[allow(clippy::too_many_arguments)]
pub fn handle_voice_fallback(
    cfg: &AiConfig,
    cmd: &str,
    vocab: &Vocab,
    apps: &mut [App],
    aliases: &mut HashMap<String, String>,
    voice_inst: Option<&Voice>,
    pill: &CornerPill,
    show_pill: bool,
    island_enabled: bool,
    app: &AppHandle,
) -> bool {
    if !cfg.enabled {
        return false;
    }
    if show_pill {
        pill.show_speech("Thinking...");
    }
    match ask_ai(
        cfg,
        cmd,
        true,
        vocab,
        apps,
        aliases,
        voice_inst,
        island_enabled,
        true,
        app,
    ) {
        Ok(out) => {
            if show_pill {
                pill.show_action(&out.reply, true);
            }
            let _ = app.emit(
                "voice-event",
                json!({
                    "type": "action",
                    "text": format!("AI: {}", out.reply),
                    "status": "AI Reply"
                }),
            );
            true
        }
        Err(err) => {
            if show_pill {
                pill.show_action(&err, false);
            }
            let _ = app.emit(
                "voice-event",
                json!({
                    "type": "action",
                    "text": format!("AI Error: {}", err),
                    "status": "AI Error"
                }),
            );
            false
        }
    }
}
