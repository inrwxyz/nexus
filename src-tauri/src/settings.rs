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
    #[serde(default)]
    pub launch_at_login: bool,
}

impl Default for ModuleState {
    fn default() -> Self {
        Self { enabled: true, launch_at_login: false }
    }
}

pub const MODULE_KEYS: [&str; 2] = ["voice", "search"];

fn default_modules() -> HashMap<String, ModuleState> {
    MODULE_KEYS
        .iter()
        .map(|k| (k.to_string(), ModuleState::default()))
        .collect()
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
}

impl Default for CircleSearchConfig {
    fn default() -> Self {
        Self {
            on_screen_trigger: false,
            hotkey: default_cs_hotkey(),
            default_action: default_cs_action(),
            target_lang: default_cs_lang(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraSettings {
    pub wake_word: String,
    pub corner_pill: bool,
    pub launch_at_login: bool,
    pub auto_updater: bool,
    #[serde(default = "default_modules")]
    pub modules: HashMap<String, ModuleState>,
    #[serde(default)]
    pub circle_search: CircleSearchConfig,
}

impl AuraSettings {
    pub fn module_enabled(&self, key: &str) -> bool {
        self.modules.get(key).map(|m| m.enabled).unwrap_or(true)
    }

    pub fn module_launch_at_login(&self, key: &str) -> bool {
        self.launch_at_login
            && self.module_enabled(key)
            && self
                .modules
                .get(key)
                .map(|m| m.launch_at_login)
                .unwrap_or(false)
    }

    pub fn module_should_run_now_with(&self, key: &str, autostart_boot: bool, unlocked: bool) -> bool {
        if !self.module_enabled(key) {
            return false;
        }
        if autostart_boot && !unlocked {
            return self.module_launch_at_login(key);
        }
        true
    }

    /// Unified runtime gate shared across all modules (`voice`, `search`).
    /// On Windows `--silent` boot before manual user interaction, only modules with both main
    /// `launch_at_login` and their own `launch_at_login` enabled should start.
    pub fn module_should_run_now(&self, key: &str) -> bool {
        self.module_should_run_now_with(key, is_autostart_boot(), is_boot_unlocked())
    }

    #[allow(dead_code)]
    pub fn any_module_launches_at_login(&self) -> bool {
        self.launch_at_login && self.modules.values().any(|m| m.enabled && m.launch_at_login)
    }

    pub fn normalize(&mut self) {
        self.modules.retain(|k, _| MODULE_KEYS.contains(&k.as_str()));
        for k in MODULE_KEYS {
            self.modules.entry(k.to_string()).or_default();
        }
        if !self.launch_at_login {
            for m in self.modules.values_mut() {
                m.launch_at_login = false;
            }
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
    }
}

impl Default for AuraSettings {
    fn default() -> Self {
        Self {
            wake_word: "hey google".to_string(),
            corner_pill: true,
            launch_at_login: false,
            auto_updater: true,
            modules: default_modules(),
            circle_search: CircleSearchConfig::default(),
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

pub fn apply_launch_at_login(enabled: bool) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };

    if let Ok(exe) = std::env::current_exe() {
        let exe_str = format!("\"{}\" --silent\0", exe.to_string_lossy());
        let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
            .encode_utf16()
            .collect();
        let value_name: Vec<u16> = "Nexus\0".encode_utf16().collect();

        unsafe {
            let mut hkey = std::mem::zeroed();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut hkey,
            ) == 0
            {
                if enabled {
                    let val_u16: Vec<u16> = exe_str.encode_utf16().collect();
                    let len_bytes = (val_u16.len() * std::mem::size_of::<u16>()) as u32;
                    let _ = RegSetValueExW(
                        hkey,
                        value_name.as_ptr(),
                        0,
                        REG_SZ,
                        val_u16.as_ptr() as *const u8,
                        len_bytes,
                    );
                } else {
                    let _ = RegDeleteValueW(hkey, value_name.as_ptr());
                }
                RegCloseKey(hkey);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_launch_at_login_gates_module_logins() {
        let mut s = AuraSettings::default();
        s.launch_at_login = false;
        for k in MODULE_KEYS {
            s.modules.get_mut(k).unwrap().launch_at_login = true;
        }

        s.normalize();
        for k in MODULE_KEYS {
            assert!(!s.module_launch_at_login(k), "{k} must turn off when master is off");
            assert!(!s.modules[k].launch_at_login);
            assert!(!s.module_should_run_now_with(k, true, false));
            assert!(s.module_should_run_now_with(k, false, false));
        }

        s.launch_at_login = true;
        s.modules.get_mut("search").unwrap().launch_at_login = true;
        s.normalize();

        // At PC boot before manual interaction: only selected enabled sections start
        assert!(s.module_should_run_now_with("search", true, false));
        assert!(!s.module_should_run_now_with("voice", true, false));

        // Disabling a section blocks its autostart even if launch_at_login was true
        s.modules.get_mut("search").unwrap().enabled = false;
        assert!(!s.module_launch_at_login("search"));
        assert!(!s.module_should_run_now_with("search", true, false));
        assert!(!s.module_should_run_now_with("search", false, false));

        // Opening Nexus manually runs all enabled sections
        assert!(s.module_should_run_now_with("voice", true, true));
    }
}
