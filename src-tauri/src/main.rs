#![windows_subsystem = "windows"]

pub mod ai;
pub mod app_launcher;
pub mod audio_native;
pub mod audio_switch;
pub mod brightness;
pub mod clipboard_sync;
pub mod native_cmds;
pub mod companion;
pub mod engine;
pub mod executor;
pub mod file_search;
pub mod gamepad;
pub mod input;
pub mod island_bridge;
pub mod media_native;
pub mod modules;
pub mod pill;
pub mod remote_terminal;
pub mod safety;
pub mod screenshot;
pub mod server;
pub mod settings;
pub mod sound;
pub mod spotify;
pub mod trackpad;
pub mod vocab;
pub mod voice;
pub mod volume;
pub mod wake_word;
pub mod window_mgr;
pub mod youtube;

pub use std::io;
pub use std::os::windows::process::CommandExt;
pub use std::process::Command;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub use app_launcher::*;
pub use engine::*;
pub use executor::*;
pub use file_search::*;
pub use input::*;
pub use island_bridge::*;
pub use vocab::*;
pub use volume::*;
pub use wake_word::*;
pub use window_mgr::*;
pub(crate) use youtube::YT_SESSION;
pub(crate) use spotify::SP_SESSION;

// Win32 re-exports for sibling modules (youtube, spotify, etc.)
pub use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT};
pub use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
pub use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
pub use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_ESCAPE,
};
pub use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW,
    SetCursorPos, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, WM_CLOSE, WS_EX_TOOLWINDOW,
};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewWindow,
};

#[cfg(target_os = "windows")]
fn trim_working_set() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

#[cfg(not(target_os = "windows"))]
fn trim_working_set() {}

#[cfg(target_os = "windows")]
fn set_webview_low_memory(window: &WebviewWindow, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    let _ = window.with_webview(move |wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            if let Ok(wv19) = windows_core_062::Interface::cast::<ICoreWebView2_19>(&core) {
                let level = if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                };
                let _ = wv19.SetMemoryUsageTargetLevel(level);
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn set_webview_low_memory(_window: &WebviewWindow, _low: bool) {}

fn hide_settings_to_tray(window: &WebviewWindow) {
    let _ = window.hide();
    set_webview_low_memory(window, true);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(350));
        trim_working_set();
    });
}

pub static MIC_MUTED: AtomicBool = AtomicBool::new(false);
static TRAY_MUTE_ITEM: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>> = Mutex::new(None);
static CACHED_UPDATE_VERSION: Mutex<Option<String>> = Mutex::new(None);

#[tauri::command]
fn check_inex_island_installed() -> bool {
    island_bridge::is_inex_island_installed()
}

fn show_settings_window(app: &AppHandle) {
    let first_unlock = !settings::is_boot_unlocked();
    settings::unlock_boot();
    if first_unlock {
        let state = app.state::<Arc<Mutex<settings::AuraSettings>>>();
        let current = state.lock().unwrap().clone();
        voice::set_listening_allowed(current.module_should_run_now("voice"));
        modules::circle_search::sync_with_settings(&current, true);
        modules::scrcpy::sync_with_settings(&current);
        gamepad::sync_with_settings(&current);
    }
    if let Some(w) = app.get_webview_window("main") {
        set_webview_low_memory(&w, false);
        let _ = w.show();

        let _ = w.unminimize();

        let _ = w.set_focus();
    }
    if let Ok(guard) = CACHED_UPDATE_VERSION.lock() {
        if let Some(ver) = guard.as_ref() {
            let _ = app.emit(
                "update-available",
                serde_json::json!({ "version": ver }),
            );
        }
    }
}

#[tauri::command]

fn get_settings(state: tauri::State<Arc<Mutex<settings::AuraSettings>>>) -> settings::AuraSettings {
    state.lock().unwrap().clone()
}

#[tauri::command]

fn save_settings(
    new_settings: settings::AuraSettings,
    state: tauri::State<Arc<Mutex<settings::AuraSettings>>>,
) -> bool {
    settings::unlock_boot();
    let mut normalized = new_settings;
    {
        let current = state.lock().unwrap();
        normalized.preserve_companion_secrets(&current);
    }
    if normalized.show_on_island && !island_bridge::is_inex_island_installed() {
        normalized.show_on_island = false;
    }
    settings::save_settings(&mut normalized);

    *state.lock().unwrap() = normalized.clone();
    if !normalized.ai.enabled {
        ai::clear_conversation();
        ai::tts::stop();
    }
    voice::set_listening_allowed(normalized.module_should_run_now("voice"));
    modules::circle_search::sync_with_settings(&normalized, true);
    modules::scrcpy::sync_with_settings(&normalized);
    gamepad::sync_with_settings(&normalized);

    true
}

#[tauri::command]
fn close_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        hide_settings_to_tray(&w);
    }
}

#[tauri::command]
fn get_companion_pairing_info(
    state: tauri::State<Arc<Mutex<settings::AuraSettings>>>,
) -> serde_json::Value {
    let s = state.lock().unwrap();
    companion::pairing_info(&s)
}

#[tauri::command]
fn generate_companion_qr(
    state: tauri::State<Arc<Mutex<settings::AuraSettings>>>,
) -> serde_json::Value {
    let mut s = state.lock().unwrap();
    companion::generate_pairing_qr(&mut s)
}

#[tauri::command]
fn unpair_companion_device(state: tauri::State<Arc<Mutex<settings::AuraSettings>>>) -> bool {
    let mut s = state.lock().unwrap();
    companion::revoke(&mut s);
    true
}

#[derive(serde::Serialize)]
struct UpdateCheck {
    available: bool,
    current_version: String,
    latest_version: String,
    error: Option<String>,
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> UpdateCheck {
    let current_version = app.package_info().version.to_string();
    use tauri_plugin_updater::UpdaterExt;
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return UpdateCheck {
                available: false,
                current_version,
                latest_version: String::new(),
                error: Some(e.to_string()),
            };
        }
    };
    match updater.check().await {
        Ok(Some(update)) => UpdateCheck {
            available: true,
            current_version,
            latest_version: update.version,
            error: None,
        },
        Ok(None) => UpdateCheck {
            available: false,
            current_version,
            latest_version: String::new(),
            error: None,
        },
        Err(e) => UpdateCheck {
            available: false,
            current_version,
            latest_version: String::new(),
            error: Some(e.to_string()),
        },
    }
}

#[tauri::command]
async fn apply_update(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    if let Some(update) = updater.check().await.map_err(|e| e.to_string())? {
        update
            .download_and_install(|_chunk_len, _content_len| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
fn get_mic_mute() -> bool {
    MIC_MUTED.load(Ordering::SeqCst)
}

fn update_tray_mute_label(muted: bool) {
    if let Ok(guard) = TRAY_MUTE_ITEM.lock() {
        if let Some(item) = guard.as_ref() {
            let label = if muted {
                "Unmute Microphone (Muted)"
            } else {
                "Mute Microphone"
            };
            let _ = item.set_text(label);
        }
    }
}

fn set_mic_mute(app: tauri::AppHandle, muted: bool) -> bool {
    MIC_MUTED.store(muted, Ordering::SeqCst);
    let _ = app.emit("mic-mute-changed", serde_json::json!({ "muted": muted }));
    update_tray_mute_label(muted);
    println!("🎙️ Microphone mute set to: {}", muted);
    muted
}

#[tauri::command]
fn toggle_mic_mute(app: tauri::AppHandle) -> bool {
    let new_state = !MIC_MUTED.load(Ordering::SeqCst);
    set_mic_mute(app, new_state)
}

#[tauri::command]
fn open_browser(url: String) {
    if !safety::url_allowed(&url) {
        return;
    }
    let _ = Command::new("explorer.exe")
        .arg(&url)
        .spawn();
}

fn main() {
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(appID: *const u16) -> i32;
    }
    let app_id: Vec<u16> = "com.inrwxyz.nexus\0".encode_utf16().collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }

    let args: Vec<String> = std::env::args().collect();
    let is_silent_arg = args
        .iter()
        .any(|a| a == "--silent" || a == "--daemon" || a == "--autostart");
    settings::set_autostart_boot(is_silent_arg);

    // At Windows login, wait until Explorer's taskbar exists (0ms wait when already booted)
    // so tray icon, hotkeys, and WebView2 initialize cleanly.
    if is_silent_arg {
        let cls: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
        let mut waited_shell = false;
        for _ in 0..16 {
            let hwnd = unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                    cls.as_ptr(),
                    std::ptr::null(),
                )
            };
            if !hwnd.is_null() {
                if waited_shell {
                    std::thread::sleep(Duration::from_secs(1));
                }
                break;
            }
            waited_shell = true;
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    let settings_state = Arc::new(Mutex::new(settings::load_settings()));

    // Background voice engine is spawned inside setup with AppHandle
    // Start HTTP server on 41780 (for external bridge & compatibility)
    server::start_server(settings_state.clone());
    // Cache the installed-app list once so phone commands never rescan the disk
    companion::warm_up();
    // Start low-latency UDP Trackpad listener on 41781 (for Phone Link)
    trackpad::start_trackpad_listener(settings_state.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let silent = args
                .iter()
                .any(|a| a == "--silent" || a == "--daemon" || a == "--autostart");
            if !silent {
                show_settings_window(app);
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(settings_state.clone())
        .invoke_handler(tauri::generate_handler![
            check_inex_island_installed,
            get_settings,
            save_settings,
            close_settings,
            get_companion_pairing_info,
            generate_companion_qr,
            unpair_companion_device,
            check_update,
            apply_update,
            get_mic_mute,
            toggle_mic_mute,
            open_browser,
            modules::circle_search::trigger_circle_search,
            modules::circle_search::overlay_ready,
            modules::circle_search::get_frozen_frame,
            modules::circle_search::close_circle_search_overlay,
            modules::circle_search::process_circle_selection,
            modules::circle_search::lens_visual_search,
            modules::circle_search::translate_selection,
            modules::circle_search::google_translate_image,
            modules::circle_search::recognize_desktop_music,
            modules::circle_search::copy_circle_text,
            modules::circle_search::search_circle_text,
            modules::circle_search::get_circle_search_status,
            modules::scrcpy::sc_devices,
            modules::scrcpy::sc_connect,
            modules::scrcpy::sc_remove_device,
            modules::scrcpy::sc_make_permanent,
            modules::scrcpy::sc_saved,
            modules::scrcpy::sc_load_prefs,
            modules::scrcpy::sc_save_prefs,
            modules::scrcpy::sc_start,
            modules::scrcpy::sc_live_update,
            modules::scrcpy::sc_stop,
            modules::scrcpy::sc_active,
            modules::scrcpy::sc_qr_start,
            modules::scrcpy::sc_qr_status,
            modules::scrcpy::sc_qr_cancel,
            modules::scrcpy::sc_kill_adb,
            gamepad::get_gamepad_status,
            gamepad::release_gamepad_inputs,
            gamepad::get_gamepad_profiles,
            gamepad::select_gamepad_profile,
            gamepad::save_gamepad_profile,
            gamepad::delete_gamepad_profile,
            gamepad::reset_gamepad_preset,
            gamepad::push_active_gamepad_profile,
            ai::commands::ai_list_keys,
            ai::commands::ai_add_key,
            ai::commands::ai_remove_key,
            ai::commands::ai_set_key_enabled,
            ai::commands::ai_update_key_model,
            ai::commands::ai_test_key,
            ai::commands::ai_local_status,
            ai::commands::ai_install_local_llm,
            ai::commands::ai_pull_local_model,
            ai::commands::ai_ask_text,
            ai::commands::ai_clear_chat,
            ai::commands::ai_list_proposals,
            ai::commands::ai_approve_proposal,
            ai::commands::ai_dismiss_proposal
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let sc = settings_state.clone();
            let initial_settings = sc.lock().unwrap().clone();

            // Self-heal Windows Run + StartupApproved\Run on every start (fixes updates/cleaners).
            let launch_on = initial_settings.launch_at_login;
            std::thread::spawn(move || {
                settings::apply_launch_at_login(launch_on);
            });

            modules::circle_search::init(handle.clone(), &initial_settings);
            modules::scrcpy::sync_with_settings(&initial_settings);
            companion::set_app_handle(handle.clone());
            gamepad::sync_with_settings(&initial_settings);
            gamepad::profiles::start_auto_detector(handle.clone(), settings_state.clone());

            if initial_settings.auto_updater {
                let handle_upd = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(6));
                    tauri::async_runtime::block_on(async move {
                        use tauri_plugin_updater::UpdaterExt;
                        if let Ok(updater) = handle_upd.updater() {
                            if let Ok(Some(update)) = updater.check().await {
                                let ver = update.version;
                                if let Ok(mut guard) = CACHED_UPDATE_VERSION.lock() {
                                    *guard = Some(ver.clone());
                                }
                                let _ = handle_upd.emit(
                                    "update-available",
                                    serde_json::json!({ "version": ver }),
                                );
                            }
                        }
                    });
                });
            }

            std::thread::spawn(move || {
                start_voice_engine(sc, handle);
            });

            // Explicitly set window icon so Windows taskbar never falls back to blank document
            if let Some(w) = app.get_webview_window("main") {
                if let Some(icon) = app.default_window_icon() {
                    let _ = w.set_icon(icon.clone());
                }
            }

            // Build Tray Menu
            let settings_item =
                MenuItem::with_id(app, "settings", "Nexus Settings", true, None::<&str>)?;
            let mute_item =
                MenuItem::with_id(app, "toggle_mute", "Mute Microphone", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Nexus", true, None::<&str>)?;

            if let Ok(mut guard) = TRAY_MUTE_ITEM.lock() {
                *guard = Some(mute_item.clone());
            }

            let menu = Menu::with_items(app, &[&settings_item, &mute_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Nexus - Ambient Voice Automation")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => show_settings_window(app),
                    "toggle_mute" => {
                        let _ = toggle_mic_mute(app.clone());
                    }
                    "quit" => {
                        gamepad::release_all();
                        modules::scrcpy::sc_stop();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,

                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_settings_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app
                .default_window_icon()
                .cloned()
                .or_else(|| tauri::image::Image::from_app_icon_resource(32).ok())
            {
                tray_builder = tray_builder.icon(icon);
            }

            match tray_builder.build(app) {
                Ok(_) => println!("✅ Tray icon created successfully"),
                Err(e) => eprintln!("❌ Tray icon failed to build: {e}"),
            }

            // If --silent was passed on autostart boot, hide window into tray; otherwise show settings.
            let is_silent = settings::is_autostart_boot();

            if is_silent {
                if let Some(w) = app.get_webview_window("main") {
                    hide_settings_to_tray(&w);
                }
            } else {
                show_settings_window(app.handle());
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();

                    if let Some(w) = window.app_handle().get_webview_window("main") {
                        hide_settings_to_tray(&w);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_words_extract_cleanly_for_all_options() {
        assert_eq!(extract_command("Hey Jarvis open chrome", true, "jarvis").as_deref(), Some("open chrome"));
        assert_eq!(extract_command("Jarvish", true, "jarvis").as_deref(), Some(""));
        assert_eq!(extract_command("ok travis lock pc", true, "jarvis").as_deref(), Some("lock pc"));

        assert_eq!(extract_command("Hey P.C.", true, "pc").as_deref(), Some(""));
        assert_eq!(extract_command("pc open spotify", true, "pc").as_deref(), Some("open spotify"));
        assert_eq!(extract_command("okay computer scroll down", true, "pc").as_deref(), Some("scroll down"));
        assert_eq!(extract_command("open topic now", true, "pc"), None);

        assert_eq!(extract_command("Alexa volume up", true, "alexa").as_deref(), Some("volume up"));
        assert_eq!(extract_command("hey alaxa", true, "alexa").as_deref(), Some(""));
        assert_eq!(extract_command("ok elixa pause", true, "alexa").as_deref(), Some("pause"));

        assert_eq!(extract_command("Hey Google pause", true, "hey google").as_deref(), Some("pause"));
    }

    #[test]
    fn search_routing_uses_browser_by_default_and_handles_explicit_browser() {
        // Neither YT nor Spotify running -> Google browser search
        assert_eq!(
            parse_search_intent_with_state("search weather in dhaka", false, false, false),
            Some(("google".to_string(), "weather in dhaka".to_string()))
        );
        // Active / "this browser" request
        assert_eq!(
            parse_search_intent_with_state("in this browser search rust lang", false, true, false),
            Some(("active_browser".to_string(), "rust lang".to_string()))
        );
        assert_eq!(
            parse_search_intent_with_state("ei browser e iphone price search koro", true, false, false),
            Some(("active_browser".to_string(), "iphone price".to_string()))
        );
        // Named browser request
        assert_eq!(
            parse_search_intent_with_state("search github on brave", false, true, false),
            Some(("brave".to_string(), "github".to_string()))
        );
        // Existing YT / Spotify behavior preserved when open
        assert_eq!(
            parse_search_intent_with_state("search lofi beats", true, false, false),
            Some(("youtube".to_string(), "lofi beats".to_string()))
        );
        assert_eq!(
            parse_search_intent_with_state("search lofi beats", false, true, false),
            Some(("spotify".to_string(), "lofi beats".to_string()))
        );
    }
}
