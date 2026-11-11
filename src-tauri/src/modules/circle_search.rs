// modules/circle_search.rs — Modular "Circle to Search" backend coordinator.
// Architecture:
//   - Sub-modules in cs/: capture.rs (BitBlt), ocr.rs (Windows.Media.Ocr),
//     hotkey.rs (RegisterHotKey), trigger_pill.rs (0-MB native Win32 handle),
//     browser.rs (Win32 ShellExecuteW launcher), shazam.rs (WASAPI loopback + Shazam).
//   - Zero idle WebView2 RAM: circle-search-overlay window is created dynamically on trigger,
//     completely .destroy()-ed on close, followed by Win32 K32EmptyWorkingSet trimming.

#[path = "cs/browser.rs"]
mod browser;
#[path = "cs/capture.rs"]
mod capture;
#[path = "cs/hotkey.rs"]
mod hotkey;
#[path = "cs/ocr.rs"]
mod ocr;
#[path = "cs/shazam.rs"]
mod shazam;
#[path = "cs/trigger_pill.rs"]
mod trigger_pill;

use crate::settings::{unlock_boot, AuraSettings};
use base64::Engine;
use serde::Serialize;
use shazam::MusicRecognitionResult;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const OVERLAY_LABEL: &str = "circle-search-overlay";

struct Runtime {
    frozen: Mutex<Option<capture::FrozenFrame>>,
    app: Mutex<Option<AppHandle>>,
    settings: Mutex<AuraSettings>,
    overlay_spawning: AtomicBool,
}

fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| Runtime {
        frozen: Mutex::new(None),
        app: Mutex::new(None),
        settings: Mutex::new(AuraSettings::default()),
        overlay_spawning: AtomicBool::new(false),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CircleSearchStatus {
    pub active: bool,
    pub enabled: bool,
    pub hotkey: String,
    pub hotkey_registered: bool,
    pub hotkey_error: Option<String>,
    pub on_screen_trigger: bool,
    pub default_action: String,
    pub target_lang: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverlayInitInfo {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub default_action: String,
    pub target_lang: String,
    pub hotkey: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionResult {
    pub width: u32,
    pub height: u32,
    pub mode: String,
    pub text: String,
    pub lines: Vec<String>,
    pub line_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualSearchResult {
    pub ok: bool,
    pub opened_direct: bool,
    pub clipboard_ready: bool,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslatedLine {
    pub source_text: String,
    pub translated_text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub bg_color: String,
    pub fg_color: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationResult {
    pub ok: bool,
    pub source_text: String,
    pub translated_text: String,
    pub detected_lang: String,
    pub target_lang: String,
    pub translate_url: String,
    pub lines: Vec<TranslatedLine>,
    pub error: Option<String>,
}

fn trigger_callback() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {
        if let Some(app) = rt().app.lock().unwrap().clone() {
            spawn_circle_search_overlay(app);
        }
    })
}

fn is_active_now(s: &AuraSettings) -> bool {
    s.module_should_run_now("search")
}

/// Called once on app startup inside Tauri setup.
pub fn init(app: AppHandle, initial_settings: &AuraSettings) {
    *rt().app.lock().unwrap() = Some(app);
    sync_with_settings(initial_settings, false);
}

/// Call whenever settings are updated interactively or after unlocking boot.
pub fn sync_with_settings(settings: &AuraSettings, user_interacted: bool) {
    if user_interacted {
        unlock_boot();
    }
    let mut normalized = settings.clone();
    normalized.normalize();
    *rt().settings.lock().unwrap() = normalized.clone();

    let active = is_active_now(&normalized);
    let cb = trigger_callback();

    let _ = hotkey::sync(active, &normalized.circle_search.hotkey, cb.clone());
    trigger_pill::set_enabled(active && normalized.circle_search.on_screen_trigger, cb);

    if !active {
        destroy_music_pill();
        if let Some(app) = rt().app.lock().unwrap().clone() {
            if app.get_webview_window(OVERLAY_LABEL).is_some() {
                hide_and_destroy_overlay(&app);
            } else {
                trim_working_set();
            }
        }
    }
}

/// Immediately release working set memory pages back to Windows when idle.
fn trim_working_set() {
    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}

fn spawn_circle_search_overlay(app: AppHandle) {
    if let Some(existing) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return;
    }
    if rt()
        .overlay_spawning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    // Wait briefly so the tiny on-screen trigger handle vanishes before BitBlt runs.
    trigger_pill::set_capture_hidden(true);
    std::thread::sleep(Duration::from_millis(25));

    let frame = match capture::capture_active_monitor() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Circle to Search capture failed: {e}");
            trigger_pill::set_capture_hidden(false);
            rt().overlay_spawning.store(false, Ordering::SeqCst);
            return;
        }
    };

    let (lx, ly, lw, lh) = frame.logical_rect();
    *rt().frozen.lock().unwrap() = Some(frame);

    let builder = WebviewWindowBuilder::new(
        &app,
        OVERLAY_LABEL,
        WebviewUrl::App("circle-search-overlay.html".into()),
    )
    .title("Nexus Circle to Search")
    .decorations(false)
    .resizable(false)
    .shadow(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .visible(false)
    .position(lx, ly)
    .inner_size(lw, lh);

    match builder.build() {
        Ok(win) => {
            win.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    *rt().frozen.lock().unwrap() = None;
                    rt().overlay_spawning.store(false, Ordering::SeqCst);
                    trigger_pill::set_capture_hidden(false);
                    trim_working_set();
                }
            });

            // Safety fallback if overlay_ready doesn't fire within 1500ms
            let app_fallback = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                rt().overlay_spawning.store(false, Ordering::SeqCst);
                if let Some(w) = app_fallback.get_webview_window(OVERLAY_LABEL) {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            });
        }
        Err(e) => {
            eprintln!("Failed to create circle-search-overlay window: {e}");
            *rt().frozen.lock().unwrap() = None;
            trigger_pill::set_capture_hidden(false);
            rt().overlay_spawning.store(false, Ordering::SeqCst);
        }
    }
}

/// Hide immediately so the default browser pops into the foreground without obstruction,
/// then asynchronously `.destroy()` the WebView2 window and trim idle RAM.
fn hide_and_destroy_overlay(app: &AppHandle) {
    *rt().frozen.lock().unwrap() = None;
    rt().overlay_spawning.store(false, Ordering::SeqCst);
    trigger_pill::set_capture_hidden(false);

    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.set_always_on_top(false);
        let _ = w.hide();
    }

    let app_c = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15));
        if let Some(w) = app_c.get_webview_window(OVERLAY_LABEL) {
            let _ = w.destroy();
        }
        trim_working_set();
        std::thread::sleep(Duration::from_millis(180));
        trim_working_set();
    });
}

/// Returns whether the Circle to Search overlay window is currently open.
pub fn is_overlay_open() -> bool {
    rt().app
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|app| app.get_webview_window(OVERLAY_LABEL))
        .is_some()
}

fn circle_like_word(w: &str) -> bool {
    matches!(
        w,
        "circle"
            | "cercle"
            | "sircle"
            | "sercla"
            | "sercal"
            | "sercle"
            | "sarcle"
            | "sarkal"
            | "sarcal"
            | "sakal"
            | "circul"
            | "circol"
            | "cycle"
            | "circus"
    ) || (w.len() >= 4 && strsim::jaro_winkler(w, "circle") >= 0.74)
}

fn search_like_word(w: &str) -> bool {
    matches!(w, "search" | "serch" | "sarach" | "seach" | "such" | "serchh")
        || (w.len() >= 4 && strsim::jaro_winkler(w, "search") >= 0.78)
}

/// Maps a normalized voice command phrase into `Some(true)` (show overlay), `Some(false)` (close overlay), or `None`.
pub fn parse_voice_overlay_command(lnorm: &str, overlay_open: bool) -> Option<bool> {
    let q = lnorm.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if q.is_empty() {
        return None;
    }

    const CS_PHRASES: &[&str] = &[
        "circle to search",
        "circle 2 search",
        "circle the search",
        "circle too search",
        "circle for search",
        "circle in search",
        "circle of search",
        "circle search",
        "circle to such",
        "cercle to search",
        "cercle search",
        "sarcle to search",
        "sarcle search",
        "sarkal to search",
        "sarkal search",
        "sercla to search",
        "sercla to serch",
        "sercal to search",
        "sakal to search",
        "sircle to search",
        "sircle search",
        "cycle to search",
        "cycle search",
        "circul to search",
        "circus to search",
    ];
    let mut has_cs = CS_PHRASES.iter().any(|p| q.contains(p));

    if !has_cs {
        let words: Vec<&str> = q.split_whitespace().collect();
        for i in 0..words.len() {
            if circle_like_word(words[i]) {
                let adj1 = i + 1 < words.len() && search_like_word(words[i + 1]);
                let adj2 = i + 2 < words.len()
                    && matches!(
                        words[i + 1],
                        "to" | "2" | "two" | "too" | "do" | "the" | "for" | "of" | "a" | "in" | "ta" | "er"
                    )
                    && search_like_word(words[i + 2]);
                if adj1 || adj2 {
                    has_cs = true;
                    break;
                }
            }
        }
    }

    if has_cs {
        let padded = format!(" {q} ");
        const OFF_TOKENS: &[&str] = &[
            " off ",
            " close ",
            " stop ",
            " hide ",
            " exit ",
            " quit ",
            " cancel ",
            " disable ",
            " bondho ",
            " bandh ",
            " band ",
            " kete ",
            " kato ",
        ];
        return Some(!OFF_TOKENS.iter().any(|tok| padded.contains(tok)));
    }

    if overlay_open
        && matches!(
            q.as_str(),
            "off" | "turn off" | "close" | "close it" | "exit" | "cancel" | "stop" | "bondho koro" | "band karo"
        )
    {
        return Some(false);
    }

    None
}

/// Opens the Circle to Search overlay on screen in response to a voice command.
pub fn open_overlay_from_voice() -> Result<(), String> {
    unlock_boot();
    {
        let s = rt().settings.lock().unwrap();
        if !s.module_enabled("search") {
            return Err("Circle to Search module is currently turned off in Settings".to_string());
        }
    }
    let app = rt()
        .app
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Circle to Search app handle is not initialized yet".to_string())?;
    std::thread::spawn(move || spawn_circle_search_overlay(app));
    Ok(())
}

/// Closes and destroys the active Circle to Search overlay from a voice command.
pub fn close_overlay_from_voice() -> bool {
    let Some(app) = rt().app.lock().unwrap().clone() else {
        return false;
    };
    hide_and_destroy_overlay(&app);
    true
}

/// Voice entrypoint called from `execute_single_command`. Returns `Ok(true)` if handled cleanly,
/// `Ok(false)` if this utterance isn't a Circle to Search command, or `Err` if blocked/unavailable.
pub fn handle_voice_command(lnorm: &str) -> Result<bool, String> {
    let Some(turn_on) = parse_voice_overlay_command(lnorm, is_overlay_open()) else {
        return Ok(false);
    };
    if turn_on {
        open_overlay_from_voice()?;
        println!("🎯 Voice: Opening Circle to Search overlay");
        Ok(true)
    } else {
        let closed = close_overlay_from_voice();
        println!("❎ Voice: Closing Circle to Search overlay ({closed})");
        Ok(closed)
    }
}

fn url_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn crop_or_full(x: u32, y: u32, width: u32, height: u32) -> Result<(u32, u32, u32, u32, Vec<u8>), String> {
    let g = rt().frozen.lock().unwrap();
    let frame = g
        .as_ref()
        .ok_or_else(|| "Frozen screen frame expired".to_string())?;
    if width == 0 || height == 0 {
        let (cw, ch, bgra) = frame.crop_bgra(0, 0, frame.width, frame.height)?;
        Ok((0, 0, cw, ch, bgra))
    } else {
        let ox = x.min(frame.width.saturating_sub(1));
        let oy = y.min(frame.height.saturating_sub(1));
        let (cw, ch, bgra) = frame.crop_bgra(ox, oy, width, height)?;
        Ok((ox, oy, cw, ch, bgra))
    }
}

fn build_lens_bridge_html(png_bytes: &[u8]) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Nexus — Google Lens</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    html, body {{
      width: 100%; height: 100%; background: #0b0e14; color: #e2e8f0;
      font-family: 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif;
      display: grid; place-items: center;
    }}
    .loader {{ display: flex; flex-direction: column; align-items: center; gap: 14px; }}
    .ring {{
      width: 36px; height: 36px; border-radius: 50%;
      border: 3px solid rgba(6, 182, 212, 0.22);
      border-top-color: #22d3ee;
      animation: spin .85s linear infinite;
    }}
    .msg {{ font-size: 13.5px; font-weight: 500; color: #94a3b8; letter-spacing: .01em; }}
    @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
    form {{ display: none; }}
  </style>
</head>
<body>
  <div class="loader">
    <div class="ring"></div>
    <div class="msg">Opening Google Lens…</div>
  </div>
  <form id="lensForm" action="https://lens.google.com/v3/upload?hl=en&ep=ccm&re=df&s=4&st={ts}" method="POST" enctype="multipart/form-data">
    <input type="file" name="encoded_image" id="fileInput" />
  </form>
  <script>
    (function () {{
      var b64 = "{b64}";
      var bin = atob(b64);
      var bytes = new Uint8Array(bin.length);
      for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      var blob = new Blob([bytes], {{ type: 'image/png' }});
      var file = new File([blob], 'capture.png', {{ type: 'image/png' }});
      var dt = new DataTransfer();
      dt.items.add(file);
      document.getElementById('fileInput').files = dt.files;
      document.getElementById('lensForm').submit();
    }})();
  </script>
</body>
</html>"#
    )
}

/// Sends Ctrl+V (pastes the clipboard image into the Lens page).
fn send_ctrl_v() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mut inputs = [key(0x11, false), key(0x56, false), key(0x56, true), key(0x11, true)];
    unsafe {
        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

async fn run_lens_search_impl(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<VisualSearchResult, String> {
    let (_, _, cw, ch, bgra) = crop_or_full(x, y, width, height)?;

    let (clip_ok, bridge_path) = tauri::async_runtime::spawn_blocking(move || {
        let clip = capture::copy_bgra_to_clipboard(cw, ch, &bgra);
        let png = capture::encode_png(cw, ch, &bgra)?;
        let html = build_lens_bridge_html(&png);
        let path = std::env::temp_dir().join("nexus_lens_bridge.html");
        std::fs::write(&path, html.as_bytes())
            .map_err(|e| format!("Failed to write nexus_lens_bridge.html: {e}"))?;
        Ok::<(bool, std::path::PathBuf), String>((clip, path))
    })
    .await
    .map_err(|e| format!("Bridge build worker failed: {e}"))??;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    hide_and_destroy_overlay(&app);

    // Direct POST to lens.google.com/v3/upload returns 403 even in Chrome.
    // So: image is already on the clipboard -> open the Lens home page -> Ctrl+V once it loads.
    let msg = if clip_ok {
        browser::open_url("https://lens.google.com/")?;
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(3200));
            send_ctrl_v();
        });
        "Opened Google Lens; pasting captured image".to_string()
    } else {
        browser::open_url(&format!("http://127.0.0.1:41780/lens-bridge?t={ts}"))?;
        "Opened Google Lens bridge in default browser".to_string()
    };

    Ok(VisualSearchResult {
        ok: true,
        opened_direct: true,
        clipboard_ready: clip_ok,
        url: bridge_path.to_string_lossy().to_string(),
        message: msg,
    })
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn trigger_circle_search(app: AppHandle) -> Result<(), String> {
    unlock_boot();
    {
        let s = rt().settings.lock().unwrap();
        if !s.module_enabled("search") {
            return Err("Circle to Search module is currently turned off".to_string());
        }
    }
    std::thread::spawn(move || spawn_circle_search_overlay(app));
    Ok(())
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle) -> Result<OverlayInitInfo, String> {
    rt().overlay_spawning.store(false, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
    }
    let (width, height, scale) = {
        let g = rt().frozen.lock().unwrap();
        match g.as_ref() {
            Some(f) => (f.width, f.height, f.scale),
            None => return Err("No frozen capture buffer available".to_string()),
        }
    };
    let s = rt().settings.lock().unwrap();
    Ok(OverlayInitInfo {
        width,
        height,
        scale,
        default_action: s.circle_search.default_action.clone(),
        target_lang: s.circle_search.target_lang.clone(),
        hotkey: s.circle_search.hotkey.clone(),
    })
}

#[tauri::command]
pub fn get_frozen_frame() -> Result<tauri::ipc::Response, String> {
    let g = rt().frozen.lock().unwrap();
    let frame = g
        .as_ref()
        .ok_or_else(|| "No frozen frame captured".to_string())?;
    Ok(tauri::ipc::Response::new(frame.to_bmp()))
}

#[tauri::command]
pub fn close_circle_search_overlay(app: AppHandle) -> Result<(), String> {
    hide_and_destroy_overlay(&app);
    Ok(())
}

#[tauri::command]
pub async fn process_circle_selection(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    mode: String,
) -> Result<SelectionResult, String> {
    let (ox, oy, cw, ch, bgra) = crop_or_full(x, y, width, height)?;
    let mode_clone = mode.clone();
    tauri::async_runtime::spawn_blocking(move || {
        match ocr::extract_text_offline(cw, ch, &bgra, ox, oy) {
            Ok(out) => SelectionResult {
                width: cw,
                height: ch,
                mode: mode_clone,
                text: out.text,
                lines: out.lines,
                line_count: out.line_count,
                error: None,
            },
            Err(e) => SelectionResult {
                width: cw,
                height: ch,
                mode: mode_clone,
                text: String::new(),
                lines: Vec::new(),
                line_count: 0,
                error: Some(e),
            },
        }
    })
    .await
    .map_err(|e| format!("OCR worker error: {e}"))
}

#[tauri::command]
pub async fn lens_visual_search(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<VisualSearchResult, String> {
    run_lens_search_impl(app, x, y, width, height).await
}

#[tauri::command]
pub async fn visual_search_selection(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<VisualSearchResult, String> {
    run_lens_search_impl(app, x, y, width, height).await
}

async fn fetch_single_gtx(
    client: &reqwest::Client,
    text: &str,
    tl: &str,
) -> Result<(String, String), String> {
    let api_url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t&q={}",
        url_encode_query(tl),
        url_encode_query(text)
    );
    let resp = client
        .get(&api_url)
        .send()
        .await
        .map_err(|e| format!("Translation network error: {e}"))?;
    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Translation parse error: {e}"))?;

    let mut translated = String::new();
    if let Some(segs) = val.get(0).and_then(|v| v.as_array()) {
        for seg in segs {
            if let Some(piece) = seg.get(0).and_then(|p| p.as_str()) {
                translated.push_str(piece);
            }
        }
    }
    let detected = val
        .get(2)
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();

    Ok((translated.trim().to_string(), detected))
}

async fn translate_single_gtx(
    client: &reqwest::Client,
    text: &str,
    tl: &str,
) -> Result<(String, String), String> {
    let (mut translated, mut detected) = fetch_single_gtx(client, text, tl).await?;

    // Retry once in lowercase if an ALL-CAPS UI heading returned unchanged
    let all_upper_word =
        text.len() > 2 && text.chars().any(|c| c.is_ascii_alphabetic()) && text.chars().all(|c| !c.is_lowercase());
    if all_upper_word && translated.eq_ignore_ascii_case(text) {
        if let Ok((tr2, det2)) = fetch_single_gtx(client, &text.to_lowercase(), tl).await {
            if !tr2.is_empty() && !tr2.eq_ignore_ascii_case(text) {
                translated = tr2;
                detected = det2;
            }
        }
    }

    if translated.is_empty() {
        translated = text.to_string();
    }
    Ok((translated, detected))
}

#[tauri::command]
pub async fn translate_selection(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    target_lang: String,
) -> Result<TranslationResult, String> {
    let tl = if target_lang.trim().is_empty() {
        "en".to_string()
    } else {
        target_lang.trim().to_string()
    };

    let (ox, oy, cw, ch, bgra) = crop_or_full(x, y, width, height)?;
    let ocr_out =
        tauri::async_runtime::spawn_blocking(move || ocr::extract_text_offline(cw, ch, &bgra, ox, oy))
            .await
            .map_err(|e| format!("OCR worker error: {e}"))??;

    let src_text = ocr_out.text.trim().to_string();
    if src_text.is_empty() || ocr_out.line_boxes.is_empty() {
        return Ok(TranslationResult {
            ok: false,
            source_text: String::new(),
            translated_text: String::new(),
            detected_lang: String::new(),
            target_lang: tl,
            translate_url: String::new(),
            lines: Vec::new(),
            error: Some("No readable text found in selected region".to_string()),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    // Deduplicate identical strings and translate each line independently over concurrent HTTP/2 calls
    // so multi-line batching can never drop or misalign headings/single words.
    let mut unique_texts: Vec<String> = Vec::new();
    let mut index_map: HashMap<String, usize> = HashMap::new();
    let mut line_indices: Vec<usize> = Vec::with_capacity(ocr_out.line_boxes.len());

    for item in &ocr_out.line_boxes {
        let idx = match index_map.get(&item.text) {
            Some(&existing) => existing,
            None => {
                let next = unique_texts.len();
                index_map.insert(item.text.clone(), next);
                unique_texts.push(item.text.clone());
                next
            }
        };
        line_indices.push(idx);
    }

    let mut translated_unique = vec![String::new(); unique_texts.len()];
    let mut detected_lang = String::from("auto");

    for chunk_start in (0..unique_texts.len()).step_by(12) {
        let chunk_end = (chunk_start + 12).min(unique_texts.len());
        let mut handles = Vec::with_capacity(chunk_end - chunk_start);
        for u_idx in chunk_start..chunk_end {
            let c = client.clone();
            let txt = unique_texts[u_idx].clone();
            let lang = tl.clone();
            handles.push(tauri::async_runtime::spawn(async move {
                (u_idx, txt.clone(), translate_single_gtx(&c, &txt, &lang).await)
            }));
        }

        for h in handles {
            if let Ok((u_idx, orig, res)) = h.await {
                match res {
                    Ok((tr, det)) => {
                        translated_unique[u_idx] = tr;
                        if detected_lang == "auto" && !det.is_empty() {
                            detected_lang = det;
                        }
                    }
                    Err(_) => {
                        translated_unique[u_idx] = orig;
                    }
                }
            }
        }
    }

    let mut lines = Vec::with_capacity(ocr_out.line_boxes.len());
    for (box_item, &u_idx) in ocr_out.line_boxes.iter().zip(line_indices.iter()) {
        let tr_line = &translated_unique[u_idx];
        lines.push(TranslatedLine {
            source_text: box_item.text.clone(),
            translated_text: if tr_line.is_empty() {
                box_item.text.clone()
            } else {
                tr_line.clone()
            },
            x: box_item.x,
            y: box_item.y,
            width: box_item.width,
            height: box_item.height,
            bg_color: box_item.bg_color.clone(),
            fg_color: box_item.fg_color.clone(),
        });
    }

    let translated_full = lines
        .iter()
        .map(|l| l.translated_text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let q_enc = url_encode_query(&src_text);
    let browser_url = format!(
        "https://translate.google.com/?sl=auto&tl={}&text={}&op=translate",
        url_encode_query(&tl),
        q_enc
    );

    Ok(TranslationResult {
        ok: !translated_full.trim().is_empty(),
        source_text: src_text,
        translated_text: translated_full,
        detected_lang,
        target_lang: tl,
        translate_url: browser_url,
        lines,
        error: None,
    })
}

fn music_pill_slot() -> &'static Mutex<Option<Arc<crate::pill::CornerPill>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<crate::pill::CornerPill>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn music_pill() -> Arc<crate::pill::CornerPill> {
    let mut g = music_pill_slot().lock().unwrap();
    if let Some(p) = g.as_ref() {
        return p.clone();
    }
    let p = Arc::new(crate::pill::CornerPill::new());
    crate::pill::spawn_pill_window(p.shared.clone());
    *g = Some(p.clone());
    p
}

fn destroy_music_pill() {
    if let Some(p) = music_pill_slot().lock().unwrap().take() {
        p.destroy();
    }
}

async fn run_desktop_music_detection() -> MusicRecognitionResult {
    let audio = match tauri::async_runtime::spawn_blocking(shazam::capture_system_audio_4s)
        .await
        .map_err(|e| format!("Audio capture worker error: {e}"))
        .and_then(|r| r)
    {
        Ok(a) => a,
        Err(e) => {
            return MusicRecognitionResult {
                found: false,
                title: String::new(),
                artist: String::new(),
                cover_url: None,
                search_url: String::new(),
                source: "loopback".to_string(),
                message: e,
            };
        }
    };

    let mut result = if audio.samples_16k.len() >= 8000 && audio.peak >= 0.0005 {
        let samples_full = audio.samples_16k;
        let sample_ms = ((samples_full.len() as f64 / 16000.0) * 1000.0).round() as u64;
        let tail_slice = if samples_full.len() > 16000 * 4 {
            Some(samples_full[samples_full.len() - 16000 * 4..].to_vec())
        } else {
            None
        };

        let mut res = match tauri::async_runtime::spawn_blocking(move || {
            shazam::build_shazam_signature(&samples_full)
        })
        .await
        {
            Ok(uri) => shazam::query_shazam(uri, sample_ms)
                .await
                .unwrap_or(MusicRecognitionResult {
                    found: false,
                    title: String::new(),
                    artist: String::new(),
                    cover_url: None,
                    search_url: String::new(),
                    source: "shazam".to_string(),
                    message: "Shazam lookup unreachable".to_string(),
                }),
            Err(_) => MusicRecognitionResult {
                found: false,
                title: String::new(),
                artist: String::new(),
                cover_url: None,
                search_url: String::new(),
                source: "shazam".to_string(),
                message: "Fingerprint worker error".to_string(),
            },
        };

        if !res.found {
            if let Some(tail) = tail_slice {
                let tail_ms = ((tail.len() as f64 / 16000.0) * 1000.0).round() as u64;
                if let Ok(uri2) = tauri::async_runtime::spawn_blocking(move || {
                    shazam::build_shazam_signature(&tail)
                })
                .await
                {
                    if let Ok(r2) = shazam::query_shazam(uri2, tail_ms).await {
                        if r2.found {
                            res = r2;
                        }
                    }
                }
            }
        }
        res
    } else {
        MusicRecognitionResult {
            found: false,
            title: String::new(),
            artist: String::new(),
            cover_url: None,
            search_url: String::new(),
            source: "loopback".to_string(),
            message: "Desktop audio is silent".to_string(),
        }
    };

    if !result.found {
        if let Some((title, artist)) =
            tauri::async_runtime::spawn_blocking(shazam::query_active_smtc_track)
                .await
                .unwrap_or(None)
        {
            let q = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} {title}")
            };
            let display = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} - {title}")
            };
            result = MusicRecognitionResult {
                found: true,
                title,
                artist,
                cover_url: None,
                search_url: format!("https://www.google.com/search?q={}", url_encode_query(&q)),
                source: "system_media".to_string(),
                message: format!("Now playing: {display}"),
            };
        }
    }

    result
}

#[tauri::command]
pub async fn recognize_desktop_music(app: AppHandle) -> Result<MusicRecognitionResult, String> {
    // Unfreeze immediately so video/audio in Instagram, browsers, or media apps keeps playing
    // live without occlusion or pause while we listen in the background.
    hide_and_destroy_overlay(&app);

    let pill = music_pill();
    pill.show_speech("Listening for music…");

    tauri::async_runtime::spawn(async move {
        // Brief settle window off async worker thread so restored players resume before capture.
        let _ = tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(240));
        })
        .await;
        let result = run_desktop_music_detection().await;

        if result.found && !result.search_url.is_empty() {
            let label = if result.artist.is_empty() {
                result.title.clone()
            } else {
                format!("{} - {}", result.title, result.artist)
            };
            pill.show_action(&label, true);
            let _ = browser::open_url(&result.search_url);
        } else {
            let miss_msg = if result.message.is_empty() {
                "No song detected".to_string()
            } else {
                result.message.clone()
            };
            pill.show_speech(&miss_msg);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1800));
                pill.hide();
            });
        }
    });

    Ok(MusicRecognitionResult {
        found: false,
        title: String::new(),
        artist: String::new(),
        cover_url: None,
        search_url: String::new(),
        source: "background".to_string(),
        message: "Unfroze screen and listening in background".to_string(),
    })
}

#[tauri::command]
pub fn copy_circle_text(text: String) -> bool {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = utf16.len() * 2;
    unsafe {
        if OpenClipboard(std::ptr::null_mut() as HWND) != 0 {
            EmptyClipboard();
            let h_mem = GlobalAlloc(0x0002, bytes);
            if !h_mem.is_null() {
                let ptr = GlobalLock(h_mem) as *mut u16;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
                    GlobalUnlock(h_mem);
                    SetClipboardData(13, h_mem);
                }
            }
            CloseClipboard();
            return true;
        }
    }
    false
}

#[tauri::command]
pub fn search_circle_text(app: AppHandle, query: String) -> Result<(), String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Empty query".to_string());
    }
    let url = format!("https://www.google.com/search?q={}", url_encode_query(trimmed));
    hide_and_destroy_overlay(&app);
    browser::open_url(&url)
}

#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String, close_overlay: bool) -> Result<(), String> {
    if close_overlay {
        hide_and_destroy_overlay(&app);
    }
    browser::open_url(&url)
}

#[tauri::command]
pub fn get_circle_search_status() -> CircleSearchStatus {
    let s = rt().settings.lock().unwrap().clone();
    let hk = hotkey::current_status();
    CircleSearchStatus {
        active: is_active_now(&s),
        enabled: s.module_enabled("search"),
        hotkey: s.circle_search.hotkey,
        hotkey_registered: hk.registered,
        hotkey_error: hk.error,
        on_screen_trigger: s.circle_search.on_screen_trigger,
        default_action: s.circle_search.default_action,
        target_lang: s.circle_search.target_lang,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_self_submitting_lens_html_bridge() {
        let html = build_lens_bridge_html(&[0x89, b'P', b'N', b'G']);
        assert!(html.contains("https://lens.google.com/v3/upload?hl=en"));
        assert!(html.contains(r#"enctype="multipart/form-data""#));
        assert!(html.contains(r#"name="encoded_image""#));
        assert!(html.contains("new DataTransfer()"));
        assert!(html.contains("document.getElementById('lensForm').submit()"));
        assert!(html.contains("iVBORw=="));
    }

    #[test]
    fn parses_circle_search_voice_on_and_off_commands() {
        assert_eq!(parse_voice_overlay_command("on circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("on sercla to serch", false), Some(true));
        assert_eq!(parse_voice_overlay_command("on cycle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("turn on circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("circle to search on", false), Some(true));
        assert_eq!(parse_voice_overlay_command("open circle to search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("circle 2 search", false), Some(true));
        assert_eq!(parse_voice_overlay_command("cercle to search", false), Some(true));

        assert_eq!(parse_voice_overlay_command("off circle to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("off sakal to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("circle to search off", false), Some(false));
        assert_eq!(parse_voice_overlay_command("close circle to search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("stop circle search", false), Some(false));
        assert_eq!(parse_voice_overlay_command("circle to search bondho koro", false), Some(false));

        assert_eq!(parse_voice_overlay_command("off", true), Some(false));
        assert_eq!(parse_voice_overlay_command("close", true), Some(false));
        assert_eq!(parse_voice_overlay_command("off", false), None);
        assert_eq!(parse_voice_overlay_command("search on youtube cat videos", false), None);
        assert_eq!(parse_voice_overlay_command("search cycle price", false), None);
        assert_eq!(parse_voice_overlay_command("open chrome", false), None);
    }
}
