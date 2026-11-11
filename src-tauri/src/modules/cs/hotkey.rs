// cs/hotkey.rs — Thread-safe Win32 RegisterHotKey listener with live rebinding.

use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, TranslateMessage, HWND_MESSAGE, MSG, WM_APP, WM_CLOSE,
    WM_DESTROY, WM_HOTKEY, WNDCLASSEXW,
};

const HOTKEY_ID: i32 = 0x4E58; // 'NX'
const WM_APP_REBIND: u32 = WM_APP + 1;

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyStatus {
    pub enabled: bool,
    pub combo: String,
    pub registered: bool,
    pub error: Option<String>,
}

struct DesiredState {
    enabled: bool,
    combo: String,
    hwnd: isize,
    registered_currently: bool,
    status: HotkeyStatus,
    callback: Option<Arc<dyn Fn() + Send + Sync>>,
}

fn state() -> &'static Mutex<DesiredState> {
    static STATE: OnceLock<Mutex<DesiredState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(DesiredState {
            enabled: false,
            combo: "Ctrl+Shift+S".to_string(),
            hwnd: 0,
            registered_currently: false,
            status: HotkeyStatus {
                enabled: false,
                combo: "Ctrl+Shift+S".to_string(),
                registered: false,
                error: None,
            },
            callback: None,
        })
    })
}

pub fn current_status() -> HotkeyStatus {
    state().lock().unwrap().status.clone()
}

/// Parse shortcut strings like "Ctrl+Shift+S", "Alt+Space", "Ctrl+Alt+X", "F9".
pub fn parse_hotkey(combo: &str) -> Result<(u32, u32, String), String> {
    let mut mods: u32 = 0;
    let mut vk: Option<u32> = None;
    let mut labels: Vec<String> = Vec::new();

    for raw in combo.split('+') {
        let tok = raw.trim();
        if tok.is_empty() {
            continue;
        }
        let low = tok.to_ascii_lowercase();
        match low.as_str() {
            "ctrl" | "control" => {
                if mods & MOD_CONTROL == 0 {
                    mods |= MOD_CONTROL;
                    labels.push("Ctrl".to_string());
                }
            }
            "alt" | "menu" => {
                if mods & MOD_ALT == 0 {
                    mods |= MOD_ALT;
                    labels.push("Alt".to_string());
                }
            }
            "shift" => {
                if mods & MOD_SHIFT == 0 {
                    mods |= MOD_SHIFT;
                    labels.push("Shift".to_string());
                }
            }
            "win" | "super" | "meta" => {
                if mods & MOD_WIN == 0 {
                    mods |= MOD_WIN;
                    labels.push("Win".to_string());
                }
            }
            _ => {
                let (code, nice) = parse_key_token(&low)?;
                vk = Some(code);
                labels.push(nice);
            }
        }
    }

    let key = vk.ok_or_else(|| "Shortcut needs a non-modifier key (e.g. Ctrl+Shift+S)".to_string())?;
    let is_fn_key = (0x70..=0x87).contains(&key);
    if mods == 0 && !is_fn_key && key != 0x2C {
        return Err("Use at least one modifier (Ctrl, Alt, Shift) or a Function key (F1-F12)".to_string());
    }
    Ok((mods, key, labels.join("+")))
}

fn parse_key_token(low: &str) -> Result<(u32, String), String> {
    if low.len() == 1 {
        let ch = low.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            let up = ch.to_ascii_uppercase();
            return Ok((up as u32, up.to_string()));
        }
        if ch.is_ascii_digit() {
            return Ok((ch as u32, ch.to_string()));
        }
    }

    if let Some(rest) = low.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Ok((0x70 + (n - 1), format!("F{n}")));
            }
        }
    }

    match low {
        "space" => Ok((0x20, "Space".to_string())),
        "tab" => Ok((0x09, "Tab".to_string())),
        "enter" | "return" => Ok((0x0D, "Enter".to_string())),
        "insert" => Ok((0x2D, "Insert".to_string())),
        "delete" | "del" => Ok((0x2E, "Delete".to_string())),
        "home" => Ok((0x24, "Home".to_string())),
        "end" => Ok((0x23, "End".to_string())),
        "pageup" | "pgup" => Ok((0x21, "PageUp".to_string())),
        "pagedown" | "pgdn" => Ok((0x22, "PageDown".to_string())),
        "printscreen" | "prtsc" => Ok((0x2C, "PrintScreen".to_string())),
        "pause" => Ok((0x13, "Pause".to_string())),
        "left" | "arrowleft" => Ok((0x25, "Left".to_string())),
        "up" | "arrowup" => Ok((0x26, "Up".to_string())),
        "right" | "arrowright" => Ok((0x27, "Right".to_string())),
        "down" | "arrowdown" => Ok((0x28, "Down".to_string())),
        "`" | "backquote" | "tilde" | "oem3" => Ok((0xC0, "`".to_string())),
        "-" | "minus" => Ok((0xBD, "-".to_string())),
        "=" | "equal" | "plus" => Ok((0xBB, "=".to_string())),
        "[" | "bracketleft" => Ok((0xDB, "[".to_string())),
        "]" | "bracketright" => Ok((0xDD, "]".to_string())),
        "\\" | "backslash" => Ok((0xDC, "\\".to_string())),
        ";" | "semicolon" => Ok((0xBA, ";".to_string())),
        "'" | "quote" => Ok((0xDE, "'".to_string())),
        "," | "comma" => Ok((0xBC, ",".to_string())),
        "." | "period" => Ok((0xBE, ".".to_string())),
        "/" | "slash" => Ok((0xBF, "/".to_string())),
        _ => Err(format!("Unsupported key: {low}")),
    }
}

pub fn sync(enabled: bool, combo: &str, callback: Arc<dyn Fn() + Send + Sync>) -> HotkeyStatus {
    let canonical = match parse_hotkey(combo) {
        Ok((_, _, norm)) => norm,
        Err(_) => combo.trim().to_string(),
    };

    let hwnd = {
        let mut s = state().lock().unwrap();
        s.enabled = enabled;
        s.combo = canonical.clone();
        s.callback = if enabled { Some(callback) } else { None };
        if !enabled {
            s.status = HotkeyStatus {
                enabled: false,
                combo: canonical,
                registered: false,
                error: None,
            };
        }
        s.hwnd
    };

    if !enabled {
        if hwnd != 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
            for _ in 0..20 {
                if state().lock().unwrap().hwnd == 0 {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(8));
            }
        }
        return current_status();
    }

    if hwnd == 0 && enabled {
        start_thread();
        // Wait briefly for message-only window registration so initial status reflects real binding.
        for _ in 0..30 {
            if state().lock().unwrap().hwnd != 0 {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
    } else if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_APP_REBIND, 0, 0);
        }
        // Wait briefly for rebind on the Win32 thread
        for _ in 0..20 {
            thread::sleep(std::time::Duration::from_millis(8));
            let st = state().lock().unwrap();
            if st.status.enabled == enabled && st.status.combo == st.combo {
                break;
            }
        }
    }

    current_status()
}

fn start_thread() {
    thread::spawn(|| unsafe {
        let class_name: Vec<u16> = "NexusCircleHotkeyWnd\0".encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let mut s = state().lock().unwrap();
            s.status.registered = false;
            s.status.error = Some("Failed to create hotkey message window".to_string());
            return;
        }

        {
            let mut s = state().lock().unwrap();
            s.hwnd = hwnd as isize;
        }

        apply_rebind(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let mut s = state().lock().unwrap();
        s.hwnd = 0;
        s.registered_currently = false;
    });
}

unsafe fn apply_rebind(hwnd: HWND) {
    let (enabled, combo, was_reg) = {
        let s = state().lock().unwrap();
        (s.enabled, s.combo.clone(), s.registered_currently)
    };

    if was_reg {
        let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
    }

    let mut err: Option<String> = None;

    if enabled {
        match parse_hotkey(&combo) {
            Ok((mods, vk, norm)) => {
                if RegisterHotKey(hwnd, HOTKEY_ID, mods | MOD_NOREPEAT, vk) != 0 {
                    let mut s = state().lock().unwrap();
                    s.combo = norm.clone();
                    s.registered_currently = true;
                    s.status = HotkeyStatus {
                        enabled: true,
                        combo: norm,
                        registered: true,
                        error: None,
                    };
                    return;
                } else {
                    let code = GetLastError();
                    err = Some(if norm.eq_ignore_ascii_case("Alt+Space") {
                        "Alt+Space may be reserved by Windows/other apps (error 1409); try Ctrl+Shift+S or Ctrl+Alt+S".to_string()
                    } else {
                        format!("Could not register {norm} (Win32 error {code}, maybe used by another app)")
                    });
                }
            }
            Err(e) => {
                err = Some(e);
            }
        }
    }

    let mut s = state().lock().unwrap();
    s.registered_currently = false;
    s.status = HotkeyStatus {
        enabled,
        combo,
        registered: false,
        error: err,
    };
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_REBIND => {
            apply_rebind(hwnd);
            0
        }
        WM_HOTKEY => {
            if wparam as i32 == HOTKEY_ID {
                let cb = state().lock().unwrap().callback.clone();
                if let Some(f) = cb {
                    thread::spawn(move || f());
                }
            }
            0
        }
        WM_CLOSE => {
            let _ = UnregisterHotKey(hwnd, HOTKEY_ID);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts_cleanly() {
        let (m1, k1, n1) = parse_hotkey("Ctrl+Shift+S").unwrap();
        assert_eq!(m1, MOD_CONTROL | MOD_SHIFT);
        assert_eq!(k1, b'S' as u32);
        assert_eq!(n1, "Ctrl+Shift+S");

        let (m2, k2, n2) = parse_hotkey("alt+space").unwrap();
        assert_eq!(m2, MOD_ALT);
        assert_eq!(k2, 0x20);
        assert_eq!(n2, "Alt+Space");

        let (m3, k3, n3) = parse_hotkey("f9").unwrap();
        assert_eq!(m3, 0);
        assert_eq!(k3, 0x78);
        assert_eq!(n3, "F9");

        assert!(parse_hotkey("S").is_err());
        assert!(parse_hotkey("Ctrl").is_err());
    }
}
