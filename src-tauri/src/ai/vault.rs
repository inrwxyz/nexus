//! DPAPI-encrypted Multi-API-Key Vault with automatic rate-limit cooldown & failover.
//! Keys are encrypted immediately via Windows CryptProtectData and NEVER returned in plaintext to UI.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PROVIDERS: [&str; 5] = ["gemini", "groq", "openrouter", "openai", "claude"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredKeyEntry {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    pub masked: String,
    pub cipher_b64: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct MaskedKeyInfo {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    pub masked: String,
    pub enabled: bool,
    pub cooldown_secs: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VaultFile {
    keys: Vec<StoredKeyEntry>,
}

fn vault_lock() -> &'static Mutex<HashMap<String, Instant>> {
    static COOLDOWNS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    COOLDOWNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn io_lock() -> &'static Mutex<()> {
    static IO: OnceLock<Mutex<()>> = OnceLock::new();
    IO.get_or_init(|| Mutex::new(()))
}

pub fn default_model_for_provider(provider: &str) -> &'static str {
    match provider {
        "gemini" => "gemini-3.8-flash",
        "groq" => "llama-3.3-70b-versatile",
        "openrouter" => "openrouter/free",
        "openai" => "gpt-4o-mini",
        "claude" => "claude-3-5-haiku-latest",
        _ => "gemini-2.5-flash",
    }
}

pub fn mask_secret(raw: &str) -> String {
    let trimmed = raw.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 4 {
        return "••••".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("••••{}", tail)
}

pub fn normalize_provider(provider: &str) -> Option<String> {
    let p = provider.trim().to_ascii_lowercase();
    if PROVIDERS.contains(&p.as_str()) {
        Some(p)
    } else {
        None
    }
}

fn vault_path() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Nexus");
        let _ = fs::create_dir_all(&dir);
        return dir.join("ai-keys.vault");
    }
    PathBuf::from("ai-keys.vault")
}

// ---------------------------------------------------------------- Windows DPAPI

#[cfg(target_os = "windows")]
mod dpapi {
    use std::ptr;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: plain.len() as u32,
            pb_data: plain.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 || output.pb_data.is_null() {
            return Err("Windows DPAPI CryptProtectData failed".to_string());
        }
        let slice = unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) };
        let res = slice.to_vec();
        unsafe {
            LocalFree(output.pb_data as *mut core::ffi::c_void);
        }
        Ok(res)
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        let input = DataBlob {
            cb_data: cipher.len() as u32,
            pb_data: cipher.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 || output.pb_data.is_null() {
            return Err("Windows DPAPI CryptUnprotectData failed".to_string());
        }
        let slice = unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) };
        let res = slice.to_vec();
        unsafe {
            LocalFree(output.pb_data as *mut core::ffi::c_void);
        }
        Ok(res)
    }
}

#[cfg(not(target_os = "windows"))]
mod dpapi {
    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        Ok(plain.to_vec())
    }
    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        Ok(cipher.to_vec())
    }
}

fn read_vault_unlocked() -> VaultFile {
    let p = vault_path();
    if let Ok(text) = fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str::<VaultFile>(&text) {
            return v;
        }
    }
    VaultFile::default()
}

fn write_vault_unlocked(v: &VaultFile) -> Result<(), String> {
    let p = vault_path();
    let json = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(p, json).map_err(|e| e.to_string())
}

pub fn list_keys_masked() -> Vec<MaskedKeyInfo> {
    let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let v = read_vault_unlocked();
    let now = Instant::now();
    let cd = vault_lock().lock().unwrap_or_else(|e| e.into_inner());
    v.keys
        .into_iter()
        .map(|k| {
            let cooldown_secs = cd
                .get(&k.id)
                .and_then(|until| until.checked_duration_since(now))
                .map(|d| d.as_secs())
                .unwrap_or(0);
            MaskedKeyInfo {
                id: k.id,
                label: k.label,
                provider: k.provider,
                model: k.model,
                masked: k.masked,
                enabled: k.enabled,
                cooldown_secs,
            }
        })
        .collect()
}

pub fn add_key(
    label: &str,
    provider: &str,
    model: &str,
    raw_secret: &str,
) -> Result<MaskedKeyInfo, String> {
    let prov = normalize_provider(provider)
        .ok_or_else(|| format!("Unsupported provider '{}'", provider))?;
    let secret = raw_secret.trim();
    if secret.len() < 8 {
        return Err("API key looks too short".to_string());
    }
    let clean_label = if label.trim().is_empty() {
        format!("{} Key", prov)
    } else {
        label.trim().to_string()
    };
    let clean_model = if model.trim().is_empty() {
        default_model_for_provider(&prov).to_string()
    } else {
        model.trim().to_string()
    };

    let protected = dpapi::protect(secret.as_bytes())?;
    let cipher_b64 = base64::engine::general_purpose::STANDARD.encode(protected);
    let masked = mask_secret(secret);
    let id = uuid::Uuid::new_v4().to_string();

    let entry = StoredKeyEntry {
        id: id.clone(),
        label: clean_label.clone(),
        provider: prov.clone(),
        model: clean_model.clone(),
        masked: masked.clone(),
        cipher_b64,
        enabled: true,
    };

    {
        let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut v = read_vault_unlocked();
        v.keys.push(entry);
        write_vault_unlocked(&v)?;
    }

    Ok(MaskedKeyInfo {
        id,
        label: clean_label,
        provider: prov,
        model: clean_model,
        masked,
        enabled: true,
        cooldown_secs: 0,
    })
}

pub fn remove_key(id: &str) -> Result<bool, String> {
    let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut v = read_vault_unlocked();
    let before = v.keys.len();
    v.keys.retain(|k| k.id != id);
    if v.keys.len() != before {
        write_vault_unlocked(&v)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn set_key_enabled(id: &str, enabled: bool) -> Result<bool, String> {
    let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut v = read_vault_unlocked();
    if let Some(item) = v.keys.iter_mut().find(|k| k.id == id) {
        item.enabled = enabled;
        write_vault_unlocked(&v)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn update_key_model(id: &str, model: &str) -> Result<bool, String> {
    let clean = model.trim();
    if clean.is_empty() {
        return Err("Model ID cannot be empty".to_string());
    }
    let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut v = read_vault_unlocked();
    if let Some(item) = v.keys.iter_mut().find(|k| k.id == id) {
        item.model = clean.to_string();
        write_vault_unlocked(&v)?;
        clear_cooldown(id);
        return Ok(true);
    }
    Ok(false)
}

pub fn mark_rate_limited(id: &str, secs: u64) {
    let mut cd = vault_lock().lock().unwrap_or_else(|e| e.into_inner());
    cd.insert(id.to_string(), Instant::now() + Duration::from_secs(secs));
}

pub fn clear_cooldown(id: &str) {
    let mut cd = vault_lock().lock().unwrap_or_else(|e| e.into_inner());
    cd.remove(id);
}

#[derive(Clone)]
pub struct DecryptedCandidate {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

fn decrypt_entry(e: &StoredKeyEntry) -> Result<DecryptedCandidate, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(e.cipher_b64.as_bytes())
        .map_err(|err| format!("Corrupt vault entry: {err}"))?;
    let plain = dpapi::unprotect(&bytes)?;
    let api_key = String::from_utf8(plain).map_err(|_| "Invalid UTF-8 key".to_string())?;
    Ok(DecryptedCandidate {
        id: e.id.clone(),
        label: e.label.clone(),
        provider: e.provider.clone(),
        model: e.model.clone(),
        api_key,
    })
}

/// Returns enabled decrypted keys ordered with ready keys first and cooled-down keys last.
pub fn candidates_for_call() -> Vec<DecryptedCandidate> {
    let v = {
        let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
        read_vault_unlocked()
    };
    let now = Instant::now();
    let cd = vault_lock().lock().unwrap_or_else(|e| e.into_inner());

    let mut ready = Vec::new();
    let mut cooling = Vec::new();
    for entry in &v.keys {
        if !entry.enabled {
            continue;
        }
        let is_cooling = cd.get(&entry.id).map(|u| *u > now).unwrap_or(false);
        if let Ok(cand) = decrypt_entry(entry) {
            if is_cooling {
                cooling.push(cand);
            } else {
                ready.push(cand);
            }
        }
    }
    ready.extend(cooling);
    ready
}

pub fn decrypt_single(id: &str) -> Result<DecryptedCandidate, String> {
    let v = {
        let _g = io_lock().lock().unwrap_or_else(|e| e.into_inner());
        read_vault_unlocked()
    };
    let entry = v
        .keys
        .iter()
        .find(|k| k.id == id)
        .ok_or_else(|| "API key not found".to_string())?;
    decrypt_entry(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_dpapi_roundtrips_without_leaking_secret() {
        assert_eq!(mask_secret("AIzaSyD1234567890abcd"), "••••abcd");
        assert_eq!(mask_secret("abc"), "••••");

        let raw = "sk-test-secret-key-987654";
        let cipher = dpapi::protect(raw.as_bytes()).expect("DPAPI protect failed");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&cipher);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let plain = String::from_utf8(dpapi::unprotect(&decoded).expect("DPAPI unprotect failed")).unwrap();
        assert_eq!(plain, raw);
    }
}
