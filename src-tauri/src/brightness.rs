// brightness.rs — native screen brightness for voice / phone commands.
//
// Same three layers Inex Island already uses (tested there), so both apps behave alike:
//   1. DDC/CI  (SetMonitorBrightness)   → external monitors
//   2. WMI     (WmiSetBrightness)       → laptop panels
//   3. Gamma ramp (SetDeviceGammaRamp)  → software dim on top, never fully black (floor 15 %)
//
// No helper processes (no powershell / cmd). Nothing stays resident: every call runs on a
// short-lived thread (clean COM state, independent of the voice thread) and exits, so the
// idle cost in RAM / CPU is zero.
//
//   brightness::handle_voice_command(lnorm) -> Option<bool>   "brightness up", "set brightness 40" ...

use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use windows_sys::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use windows_sys::Win32::Graphics::Gdi::{GetDC, MonitorFromWindow, ReleaseDC, MONITOR_DEFAULTTOPRIMARY};
use windows_sys::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

const STEP: i32 = 10;
/// Used for up / down when this PC reports no readable level (same default Island uses).
const UNKNOWN_LEVEL: u32 = 100;

#[link(name = "gdi32")]
extern "system" {
    fn SetDeviceGammaRamp(hdc: *mut c_void, ramp: *const c_void) -> i32;
}

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
struct MonitorBrightness {
    current_brightness: u8,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightnessMethods")]
struct MonitorMethods {
    #[serde(rename = "__Path")]
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SetBrightnessIn {
    timeout: u32,
    brightness: u8,
}

#[derive(Deserialize, Debug)]
struct NoOut {}

// ───────────────────────── layers ─────────────────────────

fn wmi_connect() -> Option<wmi::WMIConnection> {
    let com = wmi::COMLibrary::new().ok()?;
    wmi::WMIConnection::with_namespace_path("root\\WMI", com).ok()
}

/// Runs `f` on every physical monitor of the primary display.
unsafe fn for_each_primary_monitor<T>(mut f: impl FnMut(*mut c_void) -> Option<T>) -> Option<T> {
    let hmon = MonitorFromWindow(GetDesktopWindow(), MONITOR_DEFAULTTOPRIMARY);
    let mut count = 0u32;
    if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) == 0 || count == 0 {
        return None;
    }
    let mut mons: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); count as usize];
    if GetPhysicalMonitorsFromHMONITOR(hmon, count, mons.as_mut_ptr()) == 0 {
        return None;
    }
    let mut out = None;
    for m in &mons {
        let handle = m.hPhysicalMonitor; // packed struct: copy the field, never borrow it
        if let Some(v) = f(handle) {
            out = Some(v);
        }
    }
    DestroyPhysicalMonitors(count, mons.as_ptr());
    out
}

fn ddc_get() -> Option<u32> {
    unsafe {
        for_each_primary_monitor(|h| {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            if GetMonitorBrightness(h, &mut min, &mut cur, &mut max) == 0 || max <= min {
                return None;
            }
            Some(((cur.saturating_sub(min)) * 100 + (max - min) / 2) / (max - min))
        })
    }
}

fn ddc_set(level: u32) -> bool {
    unsafe {
        for_each_primary_monitor(|h| {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            if GetMonitorBrightness(h, &mut min, &mut cur, &mut max) == 0 || max <= min {
                return None;
            }
            let raw = min + (level * (max - min) + 50) / 100;
            if SetMonitorBrightness(h, raw) != 0 {
                Some(())
            } else {
                None
            }
        })
        .is_some()
    }
}

fn wmi_get() -> Option<u32> {
    let con = wmi_connect()?;
    let list = con.query::<MonitorBrightness>().ok()?;
    list.first().map(|b| b.current_brightness as u32)
}

fn wmi_set(level: u32) -> bool {
    let Some(con) = wmi_connect() else { return false };
    let items: Vec<MonitorMethods> = con
        .raw_query("SELECT * FROM WmiMonitorBrightnessMethods")
        .unwrap_or_default();
    let mut ok = false;
    for m in items {
        ok |= con
            .exec_instance_method::<MonitorMethods, SetBrightnessIn, NoOut>(
                &m.path,
                "WmiSetBrightness",
                SetBrightnessIn { timeout: 0, brightness: level as u8 },
            )
            .is_ok();
    }
    ok
}

fn gamma_set(level: u32) -> bool {
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() {
            return false;
        }
        // 0..100 → factor 0.15..1.0 so the screen never goes pitch black
        let factor = 0.15 + (level.min(100) as f64 / 100.0) * 0.85;
        let mut ramp = [0u16; 768];
        for i in 0..256usize {
            let v = ((i as f64) * 256.0 * factor).min(65535.0) as u16;
            ramp[i] = v;
            ramp[256 + i] = v;
            ramp[512 + i] = v;
        }
        let ok = SetDeviceGammaRamp(dc, ramp.as_ptr() as *const c_void) != 0;
        ReleaseDC(std::ptr::null_mut(), dc);
        ok
    }
}

// ───────────────────────── public API ─────────────────────────

fn on_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(f).join().ok()
}

/// Current brightness 0..100 (monitor first, then laptop panel). None if unreadable.
pub fn get() -> Option<u32> {
    on_thread(|| ddc_get().or_else(wmi_get)).flatten()
}

/// Sets brightness 0..100 through all three layers. True if the screen actually took it.
pub fn set(level: u32) -> bool {
    let level = level.min(100);
    on_thread(move || {
        let ddc = ddc_set(level);
        let gamma = gamma_set(level);
        let wmi = wmi_set(level);
        ddc || wmi || gamma
    })
    .unwrap_or(false)
}

/// Moves brightness by `delta` percent points (clamped to 0..100).
pub fn change(delta: i32) -> bool {
    let cur = get().unwrap_or(UNKNOWN_LEVEL) as i32;
    set((cur + delta).clamp(0, 100) as u32)
}

// ───────────────────────── voice parsing ─────────────────────────

fn word_number(w: &str) -> Option<u32> {
    Some(match w {
        "zero" => 0,
        "five" => 5,
        "ten" => 10,
        "fifteen" => 15,
        "twenty" => 20,
        "twentyfive" => 25,
        "thirty" => 30,
        "thirtyfive" => 35,
        "forty" => 40,
        "fortyfive" => 45,
        "fifty" => 50,
        "fiftyfive" => 55,
        "sixty" => 60,
        "sixtyfive" => 65,
        "seventy" => 70,
        "seventyfive" => 75,
        "eighty" => 80,
        "eightyfive" => 85,
        "ninety" => 90,
        "ninetyfive" => 95,
        "hundred" => 100,
        _ => return None,
    })
}

fn find_number(words: &[&str]) -> Option<u32> {
    for w in words {
        if w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty() {
            if let Ok(v) = w.parse::<u32>() {
                if v <= 100 {
                    return Some(v);
                }
            }
        } else if let Some(v) = word_number(w) {
            return Some(v);
        }
    }
    None
}

fn mentions_brightness(l: &str) -> bool {
    l.contains("brightness")
        || l.contains("bright ness")
        || l.contains("brightnes")
        || l.contains("britness")
        || l.contains("brightner")
}

/// `l` is the already-normalised command line (lower-case, punctuation → spaces).
/// Some(result) when the line is a brightness command, None otherwise.
pub fn handle_voice_command(l: &str) -> Option<bool> {
    let words: Vec<&str> = l.split_whitespace().collect();
    let has = |w: &str| words.contains(&w);

    let named = mentions_brightness(l);
    // "make it brighter" / "dimmer" work without the word "brightness"
    let comparative = has("brighter") || has("dimmer") || has("dim");
    if !named && !comparative {
        return None;
    }

    let up = has("up") || has("increase") || has("raise") || has("higher") || has("more")
        || has("brighter") || has("barao") || has("badhao") || has("bara") || has("barau");
    let down = has("down") || has("decrease") || has("lower") || has("less") || has("reduce")
        || has("dimmer") || has("dim") || has("kom") || has("kam") || has("komao") || has("kamao");

    let by = has("by");
    let number = find_number(&words);

    if let Some(n) = number {
        if by && (up || down) {
            let sign = if down && !up { -1 } else { 1 };
            return Some(change(sign * n as i32));
        }
        println!("🔆 Brightness → {n}%");
        return Some(set(n));
    }
    if has("max") || has("maximum") || has("full") {
        println!("🔆 Brightness → 100%");
        return Some(set(100));
    }
    if has("min") || has("minimum") || has("lowest") {
        println!("🔅 Brightness → 0%");
        return Some(set(0));
    }
    if up && !down {
        println!("🔆 Brightness +{STEP}%");
        return Some(change(STEP));
    }
    if down && !up {
        println!("🔅 Brightness -{STEP}%");
        return Some(change(-STEP));
    }
    None
}
