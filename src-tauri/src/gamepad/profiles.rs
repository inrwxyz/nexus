//! Gamepad HUD profiles library, PC persistence (%LOCALAPPDATA%\Nexus\gamepad-profiles.json),
//! and lightweight foreground-window auto-game detector.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadHudItem {
    pub id: String,
    pub kind: String,  // "joystick" | "button"
    #[serde(default = "default_shape")]
    pub shape: String, // "circle" | "pedal" | "pill" | "xbox" | "joystick"
    pub label: String,
    pub nx: f32,
    pub ny: f32,
    #[serde(default = "default_one")]
    pub scale: f32,
    #[serde(default = "default_alpha")]
    pub alpha: f32,
    #[serde(default)]
    pub vk: u16,
    #[serde(default = "default_accent")]
    pub accent: u64,
}

fn default_shape() -> String { "circle".to_string() }
fn default_one() -> f32 { 1.0 }
fn default_alpha() -> f32 { 0.90 }
fn default_accent() -> u64 { 0xFF262A34 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadProfile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub gyro: bool,
    #[serde(default)]
    pub match_exes: Vec<String>,
    pub items: Vec<GamepadHudItem>,
}

fn default_category() -> String { "custom".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadProfilesStore {
    pub active_profile_id: String,
    pub detected_game: Option<String>,
    pub profiles: Vec<GamepadProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectedAppEntry {
    pub name: String,
    pub exe: String,
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: &str,
    kind: &str,
    shape: &str,
    label: &str,
    nx: f32,
    ny: f32,
    scale: f32,
    alpha: f32,
    vk: u16,
    accent: u64,
) -> GamepadHudItem {
    GamepadHudItem {
        id: id.into(),
        kind: kind.into(),
        shape: shape.into(),
        label: label.into(),
        nx,
        ny,
        scale,
        alpha,
        vk,
        accent,
    }
}

pub fn built_in_presets() -> Vec<GamepadProfile> {
    vec![
        GamepadProfile {
            id: "preset_action".into(),
            name: "Action / Shooter".into(),
            category: "action".into(),
            description: "Mobile shooter HUD: Left WASD stick, right camera aim, Fire, Scope, Jump & Sprint".into(),
            gyro: false,
            match_exes: vec![
                "gta5".into(), "gtav".into(), "valorant".into(), "cs2".into(), "fortnite".into(),
                "cod".into(), "apex".into(), "cyberpunk".into(), "pubg".into(),
            ],
            items: vec![
                item("joy_wasd", "joystick", "joystick", "MOVE", 0.16, 0.67, 1.25, 0.90, 0, 0xFF1F242D),
                item("btn_run", "button", "pill", "RUN", 0.14, 0.30, 0.95, 0.86, 0xA0, 0xFF262A34),
                item("btn_use", "button", "circle", "USE", 0.63, 0.30, 0.88, 0.86, b'E' as u16, 0xFF262A34),
                item("btn_aim", "button", "circle", "AIM", 0.70, 0.51, 1.00, 0.88, 0xF2, 0xFF2D3340),
                item("btn_fire", "button", "circle", "FIRE", 0.84, 0.66, 1.46, 0.94, 0xF1, 0xFFB8332A),
                item("btn_jump", "button", "circle", "JUMP", 0.90, 0.37, 1.02, 0.88, 0x20, 0xFF2E5C48),
                item("btn_reload", "button", "circle", "RELOAD", 0.74, 0.84, 0.90, 0.86, b'R' as u16, 0xFF262A34),
                item("btn_crouch", "button", "circle", "CROUCH", 0.91, 0.84, 0.90, 0.86, b'C' as u16, 0xFF262A34),
                item("btn_esc", "button", "pill", "ESC", 0.93, 0.14, 0.74, 0.80, 0x1B, 0xFF262A34),
            ],
        },
        GamepadProfile {
            id: "preset_racing".into(),
            name: "Racing Wheel Pro".into(),
            category: "racing".into(),
            description: "Sculpted Gas & Brake racing pedals, Nitro, Drift & live Gyro tilt wheel".into(),
            gyro: true,
            match_exes: vec![
                "forza".into(), "needforspeed".into(), "nfs".into(), "asphalt".into(),
                "dirt".into(), "f1_".into(), "eurotrucks2".into(), "assetto".into(), "beamng".into(),
            ],
            items: vec![
                item("btn_left", "button", "pill", "◀ STEER", 0.12, 0.68, 1.40, 0.90, b'A' as u16, 0xFF222834),
                item("btn_right", "button", "pill", "STEER ▶", 0.27, 0.68, 1.40, 0.90, b'D' as u16, 0xFF222834),
                item("btn_look", "button", "pill", "LOOK", 0.12, 0.28, 0.88, 0.82, b'Q' as u16, 0xFF262A34),
                item("btn_cam", "button", "pill", "CAM", 0.26, 0.28, 0.88, 0.82, b'V' as u16, 0xFF262A34),
                item("btn_brake", "button", "pedal", "BRAKE", 0.71, 0.71, 1.38, 0.94, b'S' as u16, 0xFFB8332A),
                item("btn_gas", "button", "pedal", "GAS", 0.88, 0.67, 1.56, 0.96, b'W' as u16, 0xFF1F7A52),
                item("btn_drift", "button", "pill", "DRIFT", 0.72, 0.34, 1.04, 0.90, 0x20, 0xFF334155),
                item("btn_nitro", "button", "pill", "NITRO", 0.88, 0.31, 1.12, 0.92, 0xA0, 0xFF1D4ED8),
                item("btn_esc", "button", "pill", "ESC", 0.93, 0.13, 0.74, 0.80, 0x1B, 0xFF262A34),
            ],
        },
        GamepadProfile {
            id: "preset_xbox".into(),
            name: "Xbox Pro Gamepad".into(),
            category: "console".into(),
            description: "Console-style ABXY diamond cluster, LB/RB/LT/RT bumpers & smooth stick/look".into(),
            gyro: false,
            match_exes: vec![
                "fifa".into(), "fc24".into(), "fc25".into(), "tekken".into(),
                "rocketleague".into(), "eldenring".into(), "streetfighter".into(),
            ],
            items: vec![
                item("joy_ls", "joystick", "joystick", "L STICK", 0.16, 0.67, 1.28, 0.92, 0, 0xFF1F242D),
                item("btn_lt", "button", "pill", "LT", 0.11, 0.25, 0.96, 0.88, 0xF2, 0xFF262A34),
                item("btn_lb", "button", "pill", "LB", 0.24, 0.25, 0.96, 0.88, b'Q' as u16, 0xFF262A34),
                item("btn_rb", "button", "pill", "RB", 0.76, 0.25, 0.96, 0.88, b'F' as u16, 0xFF262A34),
                item("btn_rt", "button", "pill", "RT", 0.89, 0.25, 0.96, 0.88, 0xF1, 0xFF1D4ED8),
                item("btn_y", "button", "xbox", "Y", 0.82, 0.45, 1.00, 0.94, b'E' as u16, 0xFFD97706),
                item("btn_x", "button", "xbox", "X", 0.73, 0.62, 1.00, 0.94, b'R' as u16, 0xFF2563EB),
                item("btn_b", "button", "xbox", "B", 0.91, 0.62, 1.00, 0.94, b'C' as u16, 0xFFDC2626),
                item("btn_a", "button", "xbox", "A", 0.82, 0.79, 1.04, 0.96, 0x20, 0xFF16A34A),
                item("btn_back", "button", "pill", "TAB", 0.42, 0.88, 0.76, 0.80, 0x09, 0xFF262A34),
                item("btn_start", "button", "pill", "MENU", 0.58, 0.88, 0.76, 0.80, 0x1B, 0xFF262A34),
            ],
        },
    ]
}

fn store_path() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Nexus");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("gamepad-profiles.json");
    }
    PathBuf::from("gamepad-profiles.json")
}

fn load_from_disk() -> GamepadProfilesStore {
    let presets = built_in_presets();
    let path = store_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(mut st) = serde_json::from_str::<GamepadProfilesStore>(&raw) {
            let preset_ids: HashSet<String> = presets.iter().map(|p| p.id.clone()).collect();
            let existing: std::collections::HashMap<String, GamepadProfile> =
                st.profiles.iter().cloned().map(|p| (p.id.clone(), p)).collect();
            // Always keep latest built-ins (unless user customized and kept edits on existing IDs)
            let mut merged: Vec<GamepadProfile> = presets
                .into_iter()
                .map(|d| existing.get(&d.id).cloned().unwrap_or(d))
                .collect();
            for p in st.profiles {
                if !preset_ids.contains(&p.id) && !p.id.starts_with("preset_") {
                    merged.push(p);
                }
            }
            if !merged.iter().any(|p| p.id == st.active_profile_id) {
                st.active_profile_id = merged[0].id.clone();
            }
            st.profiles = merged;
            st.detected_game = None;
            return st;
        }
    }
    GamepadProfilesStore {
        active_profile_id: presets[0].id.clone(),
        detected_game: None,
        profiles: presets,
    }
}

fn save_to_disk(st: &GamepadProfilesStore) {
    if let Ok(json) = serde_json::to_string_pretty(st) {
        let _ = std::fs::write(store_path(), json);
    }
}

fn store_lock() -> &'static Mutex<GamepadProfilesStore> {
    static STORE: OnceLock<Mutex<GamepadProfilesStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(load_from_disk()))
}

fn app_handle_slot() -> &'static Mutex<Option<AppHandle>> {
    static APP: OnceLock<Mutex<Option<AppHandle>>> = OnceLock::new();
    APP.get_or_init(|| Mutex::new(None))
}

fn emit_profiles_changed(store: &GamepadProfilesStore) {
    if let Some(app) = app_handle_slot().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = app.emit("gamepad-profiles-changed", store);
    }
}

pub fn notify_phone_profile(profile_id: &str) {
    let payload = serde_json::json!({
        "type": "gamepad_profile",
        "profile_id": profile_id,
    });
    let _ = crate::trackpad::push_udp_to_phone(&payload.to_string());
}

pub fn get_store_snapshot() -> GamepadProfilesStore {
    store_lock().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn select_profile(profile_id: &str, push_to_phone: bool) -> Result<GamepadProfilesStore, String> {
    let mut g = store_lock().lock().unwrap_or_else(|e| e.into_inner());
    if !g.profiles.iter().any(|p| p.id == profile_id) {
        return Err("Profile not found".into());
    }
    g.active_profile_id = profile_id.to_string();
    save_to_disk(&g);
    let snap = g.clone();
    drop(g);

    if push_to_phone {
        notify_phone_profile(profile_id);
    }
    emit_profiles_changed(&snap);
    Ok(snap)
}

pub fn upsert_profile(profile: GamepadProfile, make_active: bool, push_to_phone: bool) -> GamepadProfilesStore {
    let mut g = store_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = g.profiles.iter().position(|p| p.id == profile.id) {
        g.profiles[pos] = profile.clone();
    } else {
        g.profiles.push(profile.clone());
    }
    if make_active {
        g.active_profile_id = profile.id.clone();
    }
    save_to_disk(&g);
    let snap = g.clone();
    drop(g);

    if push_to_phone && make_active {
        notify_phone_profile(&profile.id);
    }
    emit_profiles_changed(&snap);
    snap
}

pub fn delete_profile(profile_id: &str) -> Result<GamepadProfilesStore, String> {
    if profile_id.starts_with("preset_") {
        return Err("Built-in preset cannot be deleted (you can edit or reset it)".into());
    }
    let mut g = store_lock().lock().unwrap_or_else(|e| e.into_inner());
    g.profiles.retain(|p| p.id != profile_id);
    if g.active_profile_id == profile_id {
        g.active_profile_id = g.profiles.first().map(|p| p.id.clone()).unwrap_or_else(|| "preset_action".into());
    }
    save_to_disk(&g);
    let snap = g.clone();
    drop(g);
    emit_profiles_changed(&snap);
    Ok(snap)
}

pub fn reset_preset(profile_id: &str, push_to_phone: bool) -> Result<GamepadProfilesStore, String> {
    let preset = built_in_presets()
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| "Not a built-in preset".to_string())?;
    Ok(upsert_profile(preset, true, push_to_phone))
}

/// Inspects foreground window executable + title on Windows and returns matching profile ID if any.
pub fn match_foreground_window(exe_lower: &str, title_lower: &str, profiles: &[GamepadProfile]) -> Option<String> {
    let exe_l = exe_lower.to_ascii_lowercase();
    let title_l = title_lower.to_ascii_lowercase();
    if exe_l.is_empty() || matches!(exe_l.as_str(), "nexus.exe" | "explorer.exe" | "shellexperiencehost.exe" | "searchhost.exe") {
        return None;
    }
    for p in profiles {
        for pat in &p.match_exes {
            let k = pat.trim().to_ascii_lowercase();
            if k.is_empty() { continue; }
            if exe_l.contains(&k) || title_l.contains(&k) {
                return Some(p.id.clone());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn foreground_exe_and_title() -> Option<(String, String)> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut title_buf = [0u16; 256];
        let tlen = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if tlen > 0 {
            String::from_utf16_lossy(&title_buf[..tlen as usize])
        } else {
            String::new()
        };

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut exe = String::new();
        if pid != 0 {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !h.is_null() {
                let mut path_buf = [0u16; 512];
                let mut len = path_buf.len() as u32;
                if QueryFullProcessImageNameW(h, 0, path_buf.as_mut_ptr(), &mut len) != 0 && len > 0 {
                    let full = String::from_utf16_lossy(&path_buf[..len as usize]);
                    exe = full.rsplit('\\').next().unwrap_or(&full).to_string();
                }
                let _ = CloseHandle(h);
            }
        }
        Some((exe, title))
    }
}

#[cfg(not(target_os = "windows"))]
fn foreground_exe_and_title() -> Option<(String, String)> {
    None
}

pub fn start_auto_detector(app: AppHandle, settings_state: std::sync::Arc<Mutex<crate::settings::AuraSettings>>) {
    *app_handle_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(app);
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        thread::Builder::new()
            .name("NexusGameDetector".into())
            .spawn(move || {
                let mut last_seen_exe = String::new();
                loop {
                    thread::sleep(Duration::from_millis(1400));
                    let (enabled, auto_det) = {
                        let s = settings_state.lock().unwrap_or_else(|e| e.into_inner());
                        (
                            s.module_should_run_now("gamepad")
                                && s.module_should_run_now("companion")
                                && s.companion.auth_token.is_some(),
                            s.gamepad.auto_detect,
                        )
                    };
                    if !enabled || !auto_det {
                        last_seen_exe.clear();
                        let mut g = store_lock().lock().unwrap_or_else(|e| e.into_inner());
                        if g.detected_game.is_some() {
                            g.detected_game = None;
                            let snap = g.clone();
                            drop(g);
                            emit_profiles_changed(&snap);
                        }
                        continue;
                    }
                    let Some((exe, title)) = foreground_exe_and_title() else { continue };
                    let exe_low = exe.to_ascii_lowercase();
                    let title_low = title.to_ascii_lowercase();
                    if exe_low == last_seen_exe {
                        continue;
                    }
                    last_seen_exe = exe_low.clone();

                    let mut matched_id: Option<String> = None;
                    let mut changed_snap: Option<GamepadProfilesStore> = None;
                    {
                        let mut g = store_lock().lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(pid) = match_foreground_window(&exe_low, &title_low, &g.profiles) {
                            g.detected_game = Some(exe.clone());
                            if g.active_profile_id != pid {
                                g.active_profile_id = pid.clone();
                                save_to_disk(&g);
                                matched_id = Some(pid);
                                changed_snap = Some(g.clone());
                            }
                        }
                    }
                    if let Some(pid) = matched_id {
                        notify_phone_profile(&pid);
                    }
                    if let Some(snap) = changed_snap {
                        emit_profiles_changed(&snap);
                    }
                }
            })
            .ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_presets_cover_all_three_categories_and_match_games() {
        let presets = built_in_presets();
        assert_eq!(presets.len(), 3);
        assert!(presets.iter().any(|p| p.id == "preset_action"));
        assert!(presets.iter().any(|p| p.id == "preset_racing" && p.gyro));
        assert!(presets.iter().any(|p| p.id == "preset_xbox"));

        assert_eq!(
            match_foreground_window("gta5.exe", "Grand Theft Auto V", &presets).as_deref(),
            Some("preset_action")
        );
        assert_eq!(
            match_foreground_window("ForzaHorizon5.exe", "Forza Horizon 5", &presets).as_deref(),
            Some("preset_racing")
        );
        assert_eq!(
            match_foreground_window("fc25.exe", "EA SPORTS FC 25", &presets).as_deref(),
            Some("preset_xbox")
        );
        assert_eq!(
            match_foreground_window("nexus.exe", "Nexus", &presets),
            None
        );
    }
}
