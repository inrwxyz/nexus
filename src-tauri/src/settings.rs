use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static AUTOSTART_BOOT: AtomicBool = AtomicBool::new(false);
static BOOT_UNLOCKED: AtomicBool = AtomicBool::new(false);

pub fn set_autostart_boot(is_autostart: bool) {
    AUTOSTART_BOOT.store(is_autostart, Ordering::SeqCst);
}

pub fn is_autostart_boot() -> bool {
    AUTOSTART_BOOT.load(Ordering::SeqCst)
}

pub fn unlock_boot() {
    BOOT_UNLOCKED.store(true, Ordering::SeqCst);
}

pub fn is_boot_unlocked() -> bool {
    BOOT_UNLOCKED.load(Ordering::SeqCst)
}

fn default_true() -> bool {
    true
}

/// Per-module switches. Each module (voice, search) is independent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleState {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for ModuleState {
    fn default() -> Self {
        Self { enabled: true }
    }
}

pub const MODULE_KEYS: [&str; 5] = ["voice", "search", "companion", "scrcpy", "gamepad"];

fn default_modules() -> HashMap<String, ModuleState> {
    let mut map = HashMap::new();
    map.insert("voice".to_string(), ModuleState { enabled: true });
    map.insert("search".to_string(), ModuleState { enabled: true });
    map.insert("companion".to_string(), ModuleState { enabled: true });
    map.insert("scrcpy".to_string(), ModuleState { enabled: false });
    map.insert("gamepad".to_string(), ModuleState { enabled: true });
    map
}

fn default_gamepad_mode() -> String {
    "action".to_string()
}

fn default_gyro_sens() -> f32 {
    1.3
}

fn default_gyro_deadzone() -> f32 {
    0.08
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadConfig {
    #[serde(default = "default_gamepad_mode")]
    pub default_mode: String,
    #[serde(default = "default_sensitivity")]
    pub aim_sensitivity: f32,
    #[serde(default = "default_gyro_sens")]
    pub gyro_sensitivity: f32,
    #[serde(default = "default_gyro_deadzone")]
    pub gyro_deadzone: f32,
    #[serde(default = "default_true")]
    pub auto_sprint: bool,
    #[serde(default = "default_true")]
    pub haptics: bool,
    #[serde(default = "default_true")]
    pub auto_detect: bool,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        Self {
            default_mode: default_gamepad_mode(),
            aim_sensitivity: default_sensitivity(),
            gyro_sensitivity: default_gyro_sens(),
            gyro_deadzone: default_gyro_deadzone(),
            auto_sprint: true,
            haptics: true,
            auto_detect: true,
        }
    }
}

fn default_cs_hotkey() -> String {
    "Ctrl+Shift+S".to_string()
}

fn default_cs_action() -> String {
    "ask".to_string()
}

fn default_cs_lang() -> String {
    "bn".to_string()
}

fn default_cs_translate_mode() -> String {
    "in_place".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleSearchConfig {
    #[serde(default)]
    pub on_screen_trigger: bool,
    #[serde(default = "default_cs_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_cs_action")]
    pub default_action: String,
    #[serde(default = "default_cs_lang")]
    pub target_lang: String,
    #[serde(default = "default_cs_translate_mode")]
    pub translate_mode: String,
}

impl Default for CircleSearchConfig {
    fn default() -> Self {
        Self {
            on_screen_trigger: false,
            hotkey: default_cs_hotkey(),
            default_action: default_cs_action(),
            target_lang: default_cs_lang(),
            translate_mode: default_cs_translate_mode(),
        }
    }
}

fn default_sensitivity() -> f32 {
    1.2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionConfig {
    #[serde(default = "default_true")]
    pub voice_relay: bool,
    #[serde(default = "default_true")]
    pub touchpad: bool,
    #[serde(default = "default_true")]
    pub inertia_scrolling: bool,
    #[serde(default = "default_sensitivity")]
    pub sensitivity: f32,
    #[serde(default = "default_true")]
    pub show_on_island: bool,
    #[serde(default)]
    pub remote_terminal: bool,
    #[serde(default)]
    pub paired_device: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub pairing_code: Option<String>,
}

impl Default for CompanionConfig {
    fn default() -> Self {
        Self {
            voice_relay: true,
            touchpad: true,
            inertia_scrolling: true,
            sensitivity: 1.2,
            show_on_island: true,
            remote_terminal: false,
            paired_device: None,
            auth_token: None,
            pairing_code: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraSettings {
    #[serde(default)]
    pub setup_completed: bool,
    pub wake_word: String,
    pub corner_pill: bool,
    #[serde(default)]
    pub show_on_island: bool,
    pub launch_at_login: bool,
    pub auto_updater: bool,
    #[serde(default = "default_modules")]
    pub modules: HashMap<String, ModuleState>,
    #[serde(default)]
    pub circle_search: CircleSearchConfig,
    #[serde(default)]
    pub companion: CompanionConfig,
    #[serde(default)]
    pub gamepad: GamepadConfig,
    #[serde(default)]
    pub ai: crate::ai::AiConfig,
}

impl AuraSettings {
    pub fn module_enabled(&self, key: &str) -> bool {
        self.modules.get(key).map(|m| m.enabled).unwrap_or(true)
    }

    pub fn module_should_run_now_with(&self, key: &str, autostart_boot: bool, unlocked: bool) -> bool {
        if key == "gamepad" && !self.module_enabled("companion") {
            return false;
        }
        if !self.module_enabled(key) {
            return false;
        }
        if autostart_boot && !unlocked {
            return self.launch_at_login;
        }
        true
    }

    /// Unified runtime gate shared across all modules (`voice`, `search`).
    /// On Windows `--silent` boot before manual user interaction, enabled modules run when
    /// master `launch_at_login` is enabled.
    pub fn module_should_run_now(&self, key: &str) -> bool {
        self.module_should_run_now_with(key, is_autostart_boot(), is_boot_unlocked())
    }

    pub fn normalize(&mut self) {
        self.modules.retain(|k, _| MODULE_KEYS.contains(&k.as_str()));
        for k in MODULE_KEYS {
            self.modules.entry(k.to_string()).or_insert_with(|| ModuleState { enabled: k != "scrcpy" });
        }
        if self.circle_search.hotkey.trim().is_empty() {
            self.circle_search.hotkey = default_cs_hotkey();
        }
        if !matches!(
            self.circle_search.default_action.as_str(),
            "ask" | "ocr" | "lens" | "translate"
        ) {
            self.circle_search.default_action = default_cs_action();
        }
        if self.circle_search.target_lang.trim().is_empty() {
            self.circle_search.target_lang = default_cs_lang();
        }
        if !matches!(
            self.circle_search.translate_mode.as_str(),
            "in_place" | "google_web"
        ) {
            self.circle_search.translate_mode = default_cs_translate_mode();
        }
        if self.show_on_island && self.corner_pill {
            self.corner_pill = false;
        }
        if self.companion.sensitivity <= 0.1 || self.companion.sensitivity > 5.0 {
            self.companion.sensitivity = default_sensitivity();
        }
        if !matches!(self.gamepad.default_mode.as_str(), "action" | "racing" | "pro") {
            self.gamepad.default_mode = default_gamepad_mode();
        }
        if self.gamepad.aim_sensitivity < 0.2 || self.gamepad.aim_sensitivity > 5.0 {
            self.gamepad.aim_sensitivity = default_sensitivity();
        }
        if self.gamepad.gyro_sensitivity < 0.2 || self.gamepad.gyro_sensitivity > 5.0 {
            self.gamepad.gyro_sensitivity = default_gyro_sens();
        }
        if self.gamepad.gyro_deadzone < 0.02 || self.gamepad.gyro_deadzone > 0.45 {
            self.gamepad.gyro_deadzone = default_gyro_deadzone();
        }
        self.ai.normalize();
    }

    /// Preserves backend-owned companion pairing state when settings are saved from the UI or HTTP API.
    pub fn preserve_companion_secrets(&mut self, existing: &AuraSettings) {
        self.companion.auth_token = existing.companion.auth_token.clone();
        self.companion.paired_device = existing.companion.paired_device.clone();
        self.companion.pairing_code = existing.companion.pairing_code.clone();
    }
}

impl Default for AuraSettings {
    fn default() -> Self {
        Self {
            setup_completed: false,
            wake_word: "hey google".to_string(),
            corner_pill: true,
            show_on_island: false,
            launch_at_login: false,
            auto_updater: true,
            modules: default_modules(),
            circle_search: CircleSearchConfig::default(),
            companion: CompanionConfig::default(),
            gamepad: GamepadConfig::default(),
            ai: crate::ai::AiConfig::default(),
        }
    }
}

pub fn get_settings_path() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Nexus");
        let _ = fs::create_dir_all(&dir);
        return dir.join("nexus-settings.json");
    }
    PathBuf::from("nexus-settings.json")
}

pub fn load_settings() -> AuraSettings {
    let p = get_settings_path();
    if p.exists() {
        if let Ok(content) = fs::read_to_string(&p) {
            if let Ok(mut s) = serde_json::from_str::<AuraSettings>(&content) {
                s.normalize();
                return s;
            }
        }
    }
    let mut default_settings = AuraSettings::default();
    save_settings(&mut default_settings);
    default_settings
}

pub fn save_settings(s: &mut AuraSettings) {
    s.normalize();
    let p = get_settings_path();
    if let Ok(content) = serde_json::to_string_pretty(s) {
        let _ = fs::write(p, content);
    }
    apply_launch_at_login(s.launch_at_login);
}

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0";
const APPROVED_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run\0";
const AUTOSTART_NAME: &str = "Nexus\0";

fn autostart_exe() -> Option<String> {
    let cur = std::env::current_exe().ok()?;
    let raw = cur.to_string_lossy().to_string();
    let clean = raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string();
    if clean.to_lowercase().contains("\\target\\debug\\") {
        // Never register a dev debug binary as login autostart; prefer installed or release exe.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let installed = PathBuf::from(&local).join("Nexus").join("nexus.exe");
            if installed.exists() {
                return Some(installed.to_string_lossy().to_string());
            }
        }
        let rel = cur
            .parent()
            .and_then(|d| d.parent())
            .map(|t| t.join("release").join("nexus.exe"))?;
        if rel.exists() {
            let s = rel.to_string_lossy().to_string();
            return Some(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
        }
        return None;
    }
    Some(clean)
}

fn reg_write_value(subkey: &str, name: &str, kind: u32, data: &[u8]) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE,
    };

    let subkey_w: Vec<u16> = subkey.encode_utf16().collect();
    let name_w: Vec<u16> = name.encode_utf16().collect();
    unsafe {
        let mut hkey = std::mem::zeroed();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        ) == 0
        {
            let _ = RegSetValueExW(
                hkey,
                name_w.as_ptr(),
                0,
                kind,
                data.as_ptr(),
                data.len() as u32,
            );
            RegCloseKey(hkey);
        }
    }
}

fn reg_delete_value(subkey: &str, name: &str) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
    };

    let subkey_w: Vec<u16> = subkey.encode_utf16().collect();
    let name_w: Vec<u16> = name.encode_utf16().collect();
    unsafe {
        let mut hkey = std::mem::zeroed();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        ) == 0
        {
            let _ = RegDeleteValueW(hkey, name_w.as_ptr());
            RegCloseKey(hkey);
        }
    }
}

pub fn apply_launch_at_login(enabled: bool) {
    use windows_sys::Win32::System::Registry::{REG_BINARY, REG_SZ};

    if enabled {
        let Some(exe) = autostart_exe() else {
            return;
        };
        let exe_cmd = format!("\"{}\" --silent\0", exe);
        let val_u16: Vec<u16> = exe_cmd.encode_utf16().collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(
                val_u16.as_ptr() as *const u8,
                val_u16.len() * std::mem::size_of::<u16>(),
            )
        };
        reg_write_value(RUN_SUBKEY, AUTOSTART_NAME, REG_SZ, bytes);
        // Ensure Windows Startup Apps does not keep Nexus disabled in StartupApproved\Run.
        reg_write_value(
            APPROVED_SUBKEY,
            AUTOSTART_NAME,
            REG_BINARY,
            &[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    } else {
        reg_delete_value(RUN_SUBKEY, AUTOSTART_NAME);
        reg_delete_value(APPROVED_SUBKEY, AUTOSTART_NAME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_launch_at_login_and_module_enabled_gate_startup() {
        let mut s = AuraSettings {
            launch_at_login: false,
            ..Default::default()
        };
        s.normalize();

        for k in MODULE_KEYS {
            assert!(!s.module_should_run_now_with(k, true, false));
            let expected = k != "scrcpy";
            assert_eq!(s.module_should_run_now_with(k, false, false), expected);
        }

        s.launch_at_login = true;
        s.normalize();

        // At PC boot with master launch_at_login enabled: all enabled sections run
        assert!(s.module_should_run_now_with("search", true, false));
        assert!(s.module_should_run_now_with("voice", true, false));

        // Disabling a section blocks its startup both at boot and after unlock
        s.modules.get_mut("search").unwrap().enabled = false;
        assert!(!s.module_should_run_now_with("search", true, false));
        assert!(!s.module_should_run_now_with("search", false, false));
        assert!(!s.module_should_run_now_with("search", true, true));

        // Enabling after unlock still runs enabled sections
        assert!(s.module_should_run_now_with("voice", true, true));

        assert_eq!(s.circle_search.translate_mode, "in_place");
        s.circle_search.translate_mode = "invalid_mode".to_string();
        s.normalize();
        assert_eq!(s.circle_search.translate_mode, "in_place");
        s.circle_search.translate_mode = "google_web".to_string();
        s.normalize();
        assert_eq!(s.circle_search.translate_mode, "google_web");
    }

    #[test]
    fn save_settings_preserves_companion_pairing_secrets() {
        let mut existing = AuraSettings::default();
        existing.companion.auth_token = Some("secret-token".to_string());
        existing.companion.paired_device = Some("Pixel 9".to_string());
        existing.companion.pairing_code = Some("123456".to_string());

        let mut incoming = AuraSettings {
            corner_pill: false,
            ..Default::default()
        };
        incoming.preserve_companion_secrets(&existing);
        incoming.normalize();

        assert_eq!(incoming.companion.auth_token.as_deref(), Some("secret-token"));
        assert_eq!(incoming.companion.paired_device.as_deref(), Some("Pixel 9"));
        assert_eq!(incoming.companion.pairing_code.as_deref(), Some("123456"));
        assert!(!incoming.corner_pill);
    }
}
