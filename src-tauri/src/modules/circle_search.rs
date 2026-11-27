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
pub mod capture;
#[path = "cs/hotkey.rs"]
mod hotkey;
#[path = "cs/lens.rs"]
mod lens;
#[path = "cs/music.rs"]
mod music;
#[path = "cs/ocr.rs"]
pub mod ocr;
#[path = "cs/shazam.rs"]
mod shazam;
#[path = "cs/trigger_pill.rs"]
mod trigger_pill;
#[path = "cs/translate.rs"]
mod translate;
#[path = "cs/voice_cmd.rs"]
mod voice_cmd;

use crate::settings::{unlock_boot, AuraSettings};
use serde::Serialize;
use shazam::MusicRecognitionResult;
use translate::*;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub use voice_cmd::parse_voice_overlay_command;

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
    pub translate_mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverlayInitInfo {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub default_action: String,
    pub target_lang: String,
    pub translate_mode: String,
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
        music::destroy_music_pill();
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
pub(crate) fn hide_and_destroy_overlay(app: &AppHandle) {
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

pub(crate) fn url_encode_query(s: &str) -> String {
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

pub(crate) fn crop_or_full(x: u32, y: u32, width: u32, height: u32) -> Result<(u32, u32, u32, u32, Vec<u8>), String> {
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

#[tauri::command]
pub async fn google_translate_image(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    target_lang: String,
) -> Result<VisualSearchResult, String> {
    lens::google_translate_image_impl(app, x, y, width, height, target_lang).await
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
        translate_mode: s.circle_search.translate_mode.clone(),
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
    lens::run_lens_search_impl(app, x, y, width, height).await
}

// Translation HTTP client & batch routines modularized in cs/translate.rs

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

    let client = translate_http_client()?;

    let mut active_boxes: Vec<&ocr::OcrLineBox> = ocr_out
        .line_boxes
        .iter()
        .filter(|item| !is_ocr_noise_line(&item.text))
        .collect();
    if active_boxes.is_empty() {
        active_boxes = ocr_out.line_boxes.iter().collect();
    }

    // Normalize line breaks inside single OCR lines so chunk splitting on '\n' stays exact.
    let mut unique_texts: Vec<String> = Vec::new();
    let mut index_map: HashMap<String, usize> = HashMap::new();
    let mut line_indices: Vec<usize> = Vec::with_capacity(active_boxes.len());

    for item in &active_boxes {
        let normalized = item.text.replace(['\r', '\n'], " ").trim().to_string();
        let key = if normalized.is_empty() {
            item.text.clone()
        } else {
            normalized
        };
        let idx = match index_map.get(&key) {
            Some(&existing) => existing,
            None => {
                let next = unique_texts.len();
                index_map.insert(key.clone(), next);
                unique_texts.push(key);
                next
            }
        };
        line_indices.push(idx);
    }

    let (translated_unique, detected_lang, first_err) =
        translate_unique_lines(client, &unique_texts, &tl).await;

    let q_enc = url_encode_query(&src_text);
    let browser_url = format!(
        "https://translate.google.com/?sl=auto&tl={}&text={}&op=translate",
        url_encode_query(&tl),
        q_enc
    );

    let succeeded = translated_unique.iter().filter(|x| x.is_some()).count();
    if succeeded == 0 {
        return Ok(TranslationResult {
            ok: false,
            source_text: src_text,
            translated_text: String::new(),
            detected_lang,
            target_lang: tl,
            translate_url: browser_url,
            lines: Vec::new(),
            error: Some(
                first_err.unwrap_or_else(|| "Translation service unreachable".to_string()),
            ),
        });
    }

    let mut raw_lines = Vec::with_capacity(active_boxes.len());
    for (box_item, &u_idx) in active_boxes.iter().zip(line_indices.iter()) {
        let translated_text = translated_unique[u_idx]
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&box_item.text)
            .to_string();
        raw_lines.push(TranslatedLine {
            source_text: box_item.text.clone(),
            translated_text,
            x: box_item.x,
            y: box_item.y,
            width: box_item.width,
            height: box_item.height,
            bg_color: box_item.bg_color.clone(),
            fg_color: box_item.fg_color.clone(),
        });
    }

    let changed_count = raw_lines
        .iter()
        .filter(|l| !l.translated_text.trim().eq_ignore_ascii_case(l.source_text.trim()))
        .count();
    let lines: Vec<TranslatedLine> = if changed_count > 0 {
        raw_lines
            .into_iter()
            .filter(|l| !l.translated_text.trim().eq_ignore_ascii_case(l.source_text.trim()))
            .collect()
    } else {
        raw_lines
    };

    let translated_full = lines
        .iter()
        .map(|l| l.translated_text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

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

#[tauri::command]
pub async fn recognize_desktop_music(app: AppHandle) -> Result<MusicRecognitionResult, String> {
    music::recognize_desktop_music_impl(app).await
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
        translate_mode: s.circle_search.translate_mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_google_translate_payloads() {
        let gtx: serde_json::Value = serde_json::json!([
            [["হ্যালো\n", "Hello\n", null], ["বিশ্ব", "world", null]],
            null,
            "en"
        ]);
        let (tr1, det1) = parse_google_translate_json(&gtx).unwrap();
        assert_eq!(tr1, "হ্যালো\nবিশ্ব");
        assert_eq!(det1, "en");

        let chrome_ex: serde_json::Value = serde_json::json!([["সেটিংস মেনু", "en"]]);
        let (tr2, det2) = parse_google_translate_json(&chrome_ex).unwrap();
        assert_eq!(tr2, "সেটিংস মেনু");
        assert_eq!(det2, "en");

        let empty_payload: serde_json::Value = serde_json::json!([[], null, "en"]);
        assert!(parse_google_translate_json(&empty_payload).is_err());
    }

    #[test]
    fn lowercase_retry_triggers_only_for_unchanged_caps() {
        assert!(needs_lowercase_retry("DOWNLOAD", "DOWNLOAD"));
        assert!(!needs_lowercase_retry("DOWNLOAD", "ডাউনলোড"));
        assert!(!needs_lowercase_retry("Download", "Download"));
        assert!(!needs_lowercase_retry("OK", "OK"));
    }

    #[test]
    fn filters_ocr_noise_lines() {
        assert!(is_ocr_noise_line(""));
        assert!(is_ocr_noise_line("..."));
        assert!(is_ocr_noise_line("O"));
        assert!(is_ocr_noise_line("|"));
        assert!(!is_ocr_noise_line("1d"));
        assert!(!is_ocr_noise_line("Chrome"));
        assert!(!is_ocr_noise_line("This PC"));
    }
}
