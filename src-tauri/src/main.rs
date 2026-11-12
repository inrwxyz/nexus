#![windows_subsystem = "windows"]

mod modules;
mod pill;

mod server;

mod settings;

mod voice;

mod media_native;
mod audio_native;
mod youtube;
mod spotify;
mod safety;

use youtube::*;
use spotify::*;

use std::collections::HashMap;

use std::io::{self, BufRead, Write};

use std::os::windows::process::CommandExt;

use std::process::Command;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT};

use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_ESCAPE,
};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW,
    SetCursorPos, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, WM_CLOSE, WS_EX_TOOLWINDOW,
};

use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};

use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};

#[allow(dead_code)]
fn set_clipboard(text: &str) -> bool {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();

    let bytes = utf16.len() * 2;

    unsafe {
        if OpenClipboard(0 as HWND) != 0 {
            EmptyClipboard();

            let h_mem = GlobalAlloc(0x0002, bytes); // GMEM_MOVEABLE = 0x0002

            if !h_mem.is_null() {
                let p_mem = GlobalLock(h_mem) as *mut u16;

                if !p_mem.is_null() {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), p_mem, utf16.len());

                    GlobalUnlock(h_mem);

                    SetClipboardData(13, h_mem); // 13 = CF_UNICODETEXT
                }
            }

            CloseClipboard();

            return true;
        }
    }

    false
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WM_SYSCOMMAND: u32 = 0x0112;
const SC_MINIMIZE: usize = 0xF020;

static MIC_MUTED: AtomicBool = AtomicBool::new(false);
static TRAY_MUTE_ITEM: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>> = Mutex::new(None);
static CACHED_UPDATE_VERSION: Mutex<Option<String>> = Mutex::new(None);

// Supported wake words (includes phonetic variants for "Inex")

#[allow(dead_code)]
const WAKE_WORDS: &[&str] = &[
    "hey google",
    "ok google",
    "okay google",
    "google",
    "hey inex",
    "ok inex",
    "inex",
    "hey index",
    "ok index",
    "index", // Google often transcribes "inex" as "index"
    "hey in x",
    "in x", // Google often transcribes "inex" as "in x"
    "hey annex",
    "annex", // Google often transcribes "inex" as "annex"
    "hey innex",
    "innex",
    "hey nx",
    "nx",
    "hey computer",
    "computer",
    "hey jarvis",
    "jarvis",
    "alexa",
];

// ---------- small helpers ----------

fn ask(prompt: &str) -> String {
    print!("{prompt}");

    io::stdout().flush().ok();

    let mut s = String::new();

    io::stdin().lock().read_line(&mut s).ok();

    s
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_phrases(text: &str, phrases: &[&str]) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut ps: Vec<Vec<&str>> = phrases.iter().map(|p| p.split_whitespace().collect()).collect();
    ps.sort_by(|a, b| b.len().cmp(&a.len()));
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut hit = 0;
        for p in &ps {
            if !p.is_empty()
                && i + p.len() <= words.len()
                && p.iter().enumerate().all(|(k, w)| words[i + k] == *w)
            {
                hit = p.len();
                break;
            }
        }
        if hit > 0 { i += hit; } else { out.push(words[i]); i += 1; }
    }
    out.join(" ")
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();

    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }

            b' ' => out.push('+'),

            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }

    out
}

// 0.0 .. 1.0 similarity between what was said and a candidate name.

fn score(query: &str, cand: &str) -> f64 {
    let q = norm(query);

    let c = norm(cand);

    if q.is_empty() || c.is_empty() {
        return 0.0;
    }

    if q == c {
        return 1.0;
    }

    let mut best = strsim::jaro_winkler(&q, &c);

    for w in c.split_whitespace() {
        best = best.max(strsim::jaro_winkler(&q, w));
    }

    if q.len() >= 3 && c.contains(&q) {
        best = best.max(0.95);
    }

    if c.len() >= 3 && q.contains(&c) {
        best = best.max(0.9);
    }

    best
}

fn pick(n: usize, f: impl Fn(usize) -> f64) -> Option<(usize, f64)> {
    (0..n)
        .map(|i| (i, f(i)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

// ---------- hardware media keys ----------

fn send_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(2, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
fn send_shift_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x10,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x10,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
fn send_ctrl_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x11,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x11,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
fn send_alt_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x12, // VK_MENU (Alt)

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x12, // VK_MENU (Alt)

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

// Volume by percentage (0 to 100%)

fn set_volume_percent(percent: u32) {
    audio_native::master(audio_native::Change::Set(percent.clamp(0, 100)));
}

fn set_app_volume(app: &str, action: &str) -> bool {
    use audio_native::Change;
    let targets: Vec<&str> = if app == "youtube" {
        vec!["chrome", "msedge"]
    } else {
        vec![app]
    };
    let ch = if action == "mute" || action == "unmute" {
        Change::ToggleMute
    } else if let Some(d) = action.strip_prefix('+') {
        Change::Delta(d.parse().unwrap_or(10))
    } else if let Some(d) = action.strip_prefix('-') {
        Change::Delta(-(d.parse::<i32>().unwrap_or(10)))
    } else if let Ok(v) = action.parse::<u32>() {
        Change::Set(v.min(100))
    } else {
        return false;
    };
    let ok = audio_native::app(&targets, ch);
    if !ok {
        println!("⚠️ {app}: audio session nai (kichu baja-che na?)");
    }
    ok
}

fn parse_volume_percent(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();

    let is_explicit_volume =
        lower.contains("volume") || lower.contains("sound") || lower.contains("awaaz");

    let has_percent = lower.contains('%') || lower.contains("percent");

    let words: Vec<&str> = lower.split_whitespace().collect();

    if !is_explicit_volume && !has_percent {
        // If just a single word like "100%" or "50%" or "80"

        if words.len() == 1 {
            let clean: String = words[0].chars().filter(|c| c.is_ascii_digit()).collect();

            if let Ok(val) = clean.parse::<u32>() {
                if val <= 100 && (val % 5 == 0 || val == 100) {
                    return Some(val);
                }
            }
        }

        return None;
    }

    // Check word numbers

    for (i, &w) in words.iter().enumerate() {
        let val = match w {
            "zero" => Some(0),

            "ten" => Some(10),

            "fifteen" => Some(15),

            "twenty" => Some(20),

            "twenty-five" | "twentyfive" => Some(25),

            "thirty" => Some(30),

            "thirty-five" | "thirtyfive" => Some(35),

            "forty" => Some(40),

            "forty-five" | "fortyfive" => Some(45),

            "fifty" => Some(50),

            "fifty-five" | "fiftyfive" => Some(55),

            "sixty" => Some(60),

            "sixty-five" | "sixtyfive" => Some(65),

            "seventy" => Some(70),

            "seventy-five" | "seventyfive" => Some(75),

            "eighty" => Some(80),

            "eighty-five" | "eightyfive" => Some(85),

            "ninety" => Some(90),

            "ninety-five" | "ninetyfive" => Some(95),

            "hundred" => Some(100),

            _ => None,
        };

        if let Some(v) = val {
            if i > 0 && words[i - 1] == "one" && v == 100 {
                return Some(100);
            }

            return Some(v);
        }
    }

    // Check digit numbers: "50", "50%", "100%"

    for word in &words {
        let clean: String = word.chars().filter(|c| c.is_ascii_digit()).collect();

        if !clean.is_empty() {
            if let Ok(val) = clean.parse::<u32>() {
                if val <= 100 {
                    return Some(val);
                }
            }
        }
    }

    None
}

// Unified handler for app-specific and master system volume

fn handle_volume_command(line: &str) -> bool {
    let lnorm = norm(line);

    // 1. Check if a specific target app is mentioned

    let app_target = if lnorm.contains("spotify") {
        Some("spotify")
    } else if lnorm.contains("youtube") {
        Some("youtube")
    } else if lnorm.contains("chrome") {
        Some("chrome")
    } else if lnorm.contains("discord") {
        Some("discord")
    } else {
        None
    };

    if let Some(app) = app_target {
        // App Mute

        if lnorm.contains("mute") || lnorm.contains("chup") || lnorm.contains("silent") {
            println!("🔇 {app}: Mute toggle");

            return set_app_volume(app, "mute");
        }

        // App Volume Up: "spotify volume up", "spotify sound barao", "spotify er sound barao"

        if lnorm.contains("volume up")
            || lnorm.contains("sound up")
            || lnorm.contains("sound barao")
            || lnorm.contains("er sound barao")
            || lnorm.contains("awaaz badhao")
            || lnorm.contains("louder")
        {
            println!("🔊 {app} Volume: +15%");

            return set_app_volume(app, "+15");
        }

        // App Volume Down: "spotify volume down", "spotify sound kom koro", "spotify er sound kom koro", "sound kam koro"

        if lnorm.contains("volume down")
            || lnorm.contains("sound down")
            || lnorm.contains("sound kom koro")
            || lnorm.contains("er sound kom koro")
            || lnorm.contains("sound kam koro")
            || lnorm.contains("awaaz kam karo")
            || lnorm.contains("quieter")
        {
            println!("🔉 {app} Volume: -15%");

            return set_app_volume(app, "-15");
        }

        // App Volume %: "spotify volume 50%", "spotify 80%"

        if let Some(pct) = parse_volume_percent(&lnorm) {
            println!("🔊 Setting {app} Volume to {}%", pct);

            return set_app_volume(app, &pct.to_string());
        }
    }

    // 2. Master PC Volume percentage

    if let Some(pct) = parse_volume_percent(&lnorm) {
        println!("🔊 Setting Master Volume to {}%", pct);

        set_volume_percent(pct);

        return true;
    }

    // 3. Master Volume Up

    if lnorm.contains("volume up")
        || lnorm.contains("sound up")
        || lnorm.contains("louder")
        || lnorm.contains("awaaz badhao")
        || lnorm.contains("sound barao")
    {
        println!("🔊 Volume: +10%");
        audio_native::master(audio_native::Change::Delta(10));
        return true;
    }

    // 4. Master Volume Down

    if lnorm.contains("volume down")
        || lnorm.contains("sound down")
        || lnorm.contains("quieter")
        || lnorm.contains("awaaz kam karo")
        || lnorm.contains("sound kom koro")
        || lnorm.contains("sound kam koro")
    {
        println!("🔉 Volume: -10%");
        audio_native::master(audio_native::Change::Delta(-10));
        return true;
    }

    // 5. Master Mute

    if lnorm.contains("mute") || lnorm.contains("unmute") || lnorm.contains("silence") {
        println!("🔇 Volume: Mute toggle");
        audio_native::master(audio_native::Change::ToggleMute);
        return true;
    }

    false
}

// Mouse scroll

fn scroll(amount: i32) {
    unsafe {
        let mut input = INPUT {
            r#type: INPUT_MOUSE,

            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,

                    dy: 0,

                    mouseData: amount as u32,

                    dwFlags: MOUSEEVENTF_WHEEL,

                    time: 0,

                    dwExtraInfo: 0,
                },
            },
        };

        SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
    }
}

#[repr(C)]
#[allow(non_snake_case)]
#[allow(dead_code)]
struct LASTINPUTINFO {
    cbSize: u32,

    dwTime: u32,
}

extern "system" {

    fn MessageBeep(u_type: u32) -> i32;

    fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;

    fn GetTickCount() -> u32;

}

// Returns true if the user has typed or moved the mouse in the last 7.5 seconds

#[allow(dead_code)]
fn is_user_busy() -> bool {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,

            dwTime: 0,
        };

        if GetLastInputInfo(&mut lii) != 0 {
            let now = GetTickCount();

            let idle_ms = now.saturating_sub(lii.dwTime);

            idle_ms < 7500
        } else {
            false
        }
    }
}

// ---------- file & folder exploration ----------

#[derive(Clone, Debug)]

struct FoundItem {
    path: std::path::PathBuf,

    is_dir: bool,
}

fn search_files_and_folders(query: &str, folder_only: bool) -> Vec<FoundItem> {
    let q = norm(query);

    if q.is_empty() {
        return Vec::new();
    }

    let mut search_roots: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from(r"E:\"),
        std::path::PathBuf::from(r"D:\"),
    ];
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let home = std::path::PathBuf::from(profile);
        search_roots.push(home.join("Desktop"));
        search_roots.push(home.join("Documents"));
        search_roots.push(home.join("Downloads"));
        search_roots.push(home);
    }

    let mut results = Vec::new();

    for root_path in &search_roots {
        if !root_path.exists() {
            continue;
        }

        scan_dir(root_path, &q, folder_only, 0, 3, &mut results);

        if results.len() >= 15 {
            break;
        }
    }

    results.sort_by(|a, b| {
        let a_str = a.path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let b_str = b.path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let a_norm = norm(a_str);

        let b_norm = norm(b_str);

        let bonus_a = if a_norm == q {
            3.0
        } else if a_norm.starts_with(&q) {
            1.8
        } else if a_norm.contains(&format!(" {q}")) {
            1.4
        } else {
            0.0
        };

        let bonus_b = if b_norm == q {
            3.0
        } else if b_norm.starts_with(&q) {
            1.8
        } else if b_norm.contains(&format!(" {q}")) {
            1.4
        } else {
            0.0
        };

        let sa = score(&q, &a_norm) + bonus_a;

        let sb = score(&q, &b_norm) + bonus_b;

        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(8);

    results
}

fn scan_dir(
    dir: &std::path::Path,

    query: &str,

    folder_only: bool,

    depth: usize,

    max_depth: usize,

    results: &mut Vec<FoundItem>,
) {
    if depth >= max_depth || results.len() >= 20 {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,

        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let is_dir = path.is_dir();

        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "AppData"
                || name == "plugins"
                || name == "venv"
                || name.contains("IntelliJ")
                || name.contains("AndroidStudio")
                || name == "$RECYCLE.BIN"
                || name == "System Volume Information"
                || name == "Windows"
                || name == "Program Files"
                || name == "Program Files (x86)"
            {
                continue;
            }

            let name_norm = norm(name);

            let matched = if query.len() <= 3 {
                name_norm == query
                    || name_norm.starts_with(&format!("{query} "))
                    || name_norm.contains(&format!(" {query}"))
                    || name_norm.starts_with(query)
            } else {
                name_norm.contains(query) || score(query, &name_norm) >= 0.75
            };

            if matched {
                if !folder_only || is_dir {
                    if !results.iter().any(|r| r.path == path) {
                        results.push(FoundItem {
                            path: path.clone(),

                            is_dir,
                        });
                    }
                }
            }

            if is_dir && depth + 1 < max_depth {
                scan_dir(&path, query, folder_only, depth + 1, max_depth, results);
            }
        }
    }
}

fn handle_file_explorer(cmd: &str, voice_inst: Option<&voice::Voice>) -> bool {
    let norm_cmd = norm(cmd);

    // Never intercept CLOSE commands

    if norm_cmd.starts_with("close ")
        || norm_cmd.starts_with("band ")
        || norm_cmd.starts_with("quit ")
        || norm_cmd.starts_with("cut ")
        || norm_cmd.starts_with("kete ")
    {
        return false;
    }

    // Standard shell folders

    if norm_cmd == "open downloads" || norm_cmd == "downloads" || norm_cmd == "downloads folder" {
        println!("📁 Opening Downloads folder...");

        let _ = Command::new("explorer.exe").arg("shell:Downloads").spawn();

        return true;
    }

    if norm_cmd == "open documents" || norm_cmd == "documents" || norm_cmd == "documents folder" {
        println!("📁 Opening Documents folder...");

        let _ = Command::new("explorer.exe").arg("shell:Personal").spawn();

        return true;
    }

    if norm_cmd == "open desktop" || norm_cmd == "desktop" || norm_cmd == "desktop folder" {
        println!("📁 Opening Desktop folder...");

        let _ = Command::new("explorer.exe").arg("shell:Desktop").spawn();

        return true;
    }

    if norm_cmd == "open pictures" || norm_cmd == "pictures" || norm_cmd == "photos folder" {
        println!("📁 Opening Pictures folder...");

        let _ = Command::new("explorer.exe")
            .arg("shell:My Pictures")
            .spawn();

        return true;
    }

    if norm_cmd == "open videos" || norm_cmd == "videos folder" {
        println!("📁 Opening Videos folder...");

        let _ = Command::new("explorer.exe").arg("shell:My Video").spawn();

        return true;
    }

    if norm_cmd == "open c drive" || norm_cmd == "c drive" {
        println!("📁 Opening C: Drive...");

        let _ = Command::new("explorer.exe").arg(r"C:\").spawn();

        return true;
    }

    if norm_cmd == "open d drive" || norm_cmd == "d drive" {
        println!("📁 Opening D: Drive...");

        let _ = Command::new("explorer.exe").arg(r"D:\").spawn();

        return true;
    }

    if norm_cmd == "open e drive" || norm_cmd == "e drive" {
        println!("📁 Opening E: Drive...");

        let _ = Command::new("explorer.exe").arg(r"E:\").spawn();

        return true;
    }

    if norm_cmd == "open file explorer" || norm_cmd == "open explorer" || norm_cmd == "explorer" {
        println!("📁 Opening File Explorer...");

        let _ = Command::new("explorer.exe").spawn();

        return true;
    }

    // Custom folder / file search: ONLY match if explicit file/folder keywords are used!

    // Never intercept generic application commands (e.g. "open chrome") or web search!

    let is_folder_req = norm_cmd.contains("folder") || norm_cmd.contains("directory");

    let is_file_req = norm_cmd.contains("file") || norm_cmd.contains("document");

    let has_file_prefix = norm_cmd.starts_with("open folder ")
        || norm_cmd.starts_with("find folder ")
        || norm_cmd.starts_with("search folder ")
        || norm_cmd.starts_with("folder ")
        || norm_cmd.starts_with("open file ")
        || norm_cmd.starts_with("find file ")
        || norm_cmd.starts_with("search file ")
        || norm_cmd.starts_with("file ")
        || ((norm_cmd.starts_with("find ") || norm_cmd.starts_with("search "))
            && (is_folder_req || is_file_req));

    if !has_file_prefix && !is_folder_req && !is_file_req {
        return false;
    }

    // Clean query

    let mut clean = norm_cmd;

    for prefix in &[
        "open folder ",
        "find folder ",
        "search folder ",
        "folder ",
        "open file ",
        "find file ",
        "search file ",
        "file ",
        "find ",
        "search ",
        "kholo ",
        "dikhau ",
    ] {
        if clean.starts_with(prefix) {
            clean = clean[prefix.len()..].to_string();

            break;
        }
    }

    for suffix in &[" folder", " file", " kholo", " karo", " koro", " dhoondo"] {
        if clean.ends_with(suffix) {
            clean = clean[..clean.len() - suffix.len()].to_string();

            break;
        }
    }

    let query = clean.trim();

    if query.is_empty()
        || query == "folder"
        || query == "file"
        || query == "youtube"
        || query == "spotify"
        || query == "chrome"
    {
        return false;
    }

    println!("🔍 Searching PC for: \"{}\"...", query);

    let items = search_files_and_folders(query, is_folder_req && !is_file_req);

    if items.is_empty() {
        println!("❌ No files or folders found matching \"{}\".", query);

        return false;
    }

    // If only 1 found: open immediately!

    if items.len() == 1 {
        let item = &items[0];

        let p_str = item.path.to_string_lossy();

        println!("📁 Found: {p_str}");

        if item.is_dir {
            let _ = Command::new("explorer.exe").arg(&*p_str).spawn();
        } else {
            let _ = Command::new("explorer.exe")
                .args(["/select,", &*p_str])
                .spawn();
        }

        return true;
    }

    // Multiple found: show list and let user pick via voice

    let count = items.len().min(5);

    println!("\nFound {count} matching items for \"{query}\":");

    for (i, it) in items.iter().take(count).enumerate() {
        let kind = if it.is_dir {
            "📁 [Folder]"
        } else {
            "📄 [File]"
        };

        println!("  {}) {} {}", i + 1, kind, it.path.to_string_lossy());
    }

    let mut choice_idx = None;

    if let Some(v) = voice_inst {
        println!("🎤 Say 'number 1', 'number 2', etc. (or press Enter for #1):");

        print!("\x07");

        io::stdout().flush().ok();

        v.reset_buffers();

        if let Ok(Some(heard)) = v.wait_for_utterance() {
            let h = norm(&heard.text);

            println!("🗣️  Choice: \"{}\"", heard.text);

            if h.contains("one") || h.contains("1") || h.contains("first") {
                choice_idx = Some(0);
            } else if h.contains("two") || h.contains("2") || h.contains("second") {
                if count >= 2 {
                    choice_idx = Some(1);
                }
            } else if h.contains("three") || h.contains("3") || h.contains("third") {
                if count >= 3 {
                    choice_idx = Some(2);
                }
            } else if h.contains("four") || h.contains("4") {
                if count >= 4 {
                    choice_idx = Some(3);
                }
            } else if h.contains("five") || h.contains("5") {
                if count >= 5 {
                    choice_idx = Some(4);
                }
            }
        }
    }

    let choice = choice_idx.unwrap_or(0);

    let chosen = &items[choice];

    let p_str = chosen.path.to_string_lossy();

    println!("📁 Opening: {p_str}");

    if chosen.is_dir {
        let _ = Command::new("explorer.exe").arg(&*p_str).spawn();
    } else {
        let _ = Command::new("explorer.exe")
            .args(["/select,", &*p_str])
            .spawn();
    }

    true
}

// ---------- vocab + aliases ----------

struct Vocab {
    actions: Vec<(String, Vec<String>)>,

    ignore: Vec<String>,
}

fn read_project_file(name: &str) -> Option<String> {
    if let Ok(s) = std::fs::read_to_string(name) {
        return Some(s);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(s) = std::fs::read_to_string(dir.join(name)) {
                return Some(s);
            }

            if let Some(parent) = dir.parent() {
                if let Ok(s) = std::fs::read_to_string(parent.join(name)) {
                    return Some(s);
                }

                if let Some(root) = parent.parent() {
                    if let Ok(s) = std::fs::read_to_string(root.join(name)) {
                        return Some(s);
                    }
                }
            }
        }
    }

    None
}

fn load_vocab() -> Vocab {
    let default_vocab = include_str!("../vocab.json");

    let txt = read_project_file("vocab.json").unwrap_or_else(|| default_vocab.to_string());

    let v: serde_json::Value =
        serde_json::from_str(&txt).unwrap_or_else(|_| serde_json::from_str(default_vocab).unwrap());

    let words = |val: &serde_json::Value| -> Vec<String> {
        val.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(norm).collect())
            .unwrap_or_default()
    };

    let mut actions = Vec::new();

    if let Some(obj) = v["actions"].as_object() {
        for (k, arr) in obj {
            actions.push((k.clone(), words(arr)));
        }
    }

    Vocab {
        actions,

        ignore: words(&v["ignore"]),
    }
}

fn load_aliases() -> HashMap<String, String> {
    read_project_file("aliases.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_aliases(a: &HashMap<String, String>) {
    if let Ok(t) = serde_json::to_string_pretty(a) {
        let _ = std::fs::write("aliases.json", t);
    }
}

// ---------- parsing ----------

struct Parsed {
    action: String,

    target: String,
}

fn parse(text: &str, v: &Vocab) -> Option<Parsed> {
    let tokens: Vec<String> = norm(text).split_whitespace().map(String::from).collect();

    let mut best: Option<(usize, usize, String)> = None;

    for (action, phrases) in &v.actions {
        for p in phrases {
            let pt: Vec<&str> = p.split_whitespace().collect();

            if pt.is_empty() || tokens.len() < pt.len() {
                continue;
            }

            for i in 0..=(tokens.len() - pt.len()) {
                let hit = tokens[i..i + pt.len()]
                    .iter()
                    .map(|s| s.as_str())
                    .eq(pt.iter().copied());

                if hit {
                    let better = match &best {
                        None => true,

                        Some((bp, bl, _)) => i < *bp || (i == *bp && pt.len() > *bl),
                    };

                    if better {
                        best = Some((i, pt.len(), action.clone()));
                    }

                    break;
                }
            }
        }
    }

    let (pos, len, action) = best?;

    let rest: Vec<&String> = tokens
        .iter()
        .enumerate()
        .filter(|(i, t)| (*i < pos || *i >= pos + len) && !v.ignore.contains(*t))
        .map(|(_, t)| t)
        .collect();

    Some(Parsed {
        action,

        target: rest
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    })
}

// ---------- installed apps (auto scan) ----------

struct App {
    name: String,

    id: String,
}

fn scan_apps() -> Vec<App> {
    let mut apps = Vec::new();

    let mut scan_dir = |base: std::path::PathBuf| {
        if !base.exists() {
            return;
        }
        let mut dirs = vec![base];
        while let Some(d) = dirs.pop() {
            if let Ok(entries) = std::fs::read_dir(&d) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dirs.push(path);
                    } else if path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("lnk"))
                        .unwrap_or(false)
                    {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let name = stem.trim().to_string();
                            let id = path.to_string_lossy().to_string();
                            if !name.is_empty() && safety::app_allowed(&name, &id) {
                                apps.push(App { name, id });
                            }
                        }
                    }
                }
            }
        }
    };

    if let Ok(appdata) = std::env::var("APPDATA") {
        scan_dir(std::path::PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Ok(programdata) = std::env::var("ProgramData") {
        scan_dir(std::path::PathBuf::from(programdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }

    let builtins = [
        ("Calculator", "calculator:"),
        ("Camera", "microsoft.windows.camera:"),
        ("Paint", "ms-paint:"),
        ("Photos", "ms-photos:"),
        ("Clock", "ms-clock:"),
        ("Notepad", "notepad.exe"),
    ];
    for (name, id) in builtins {
        if safety::app_allowed(name, id) {
            apps.push(App {
                name: name.to_string(),
                id: id.to_string(),
            });
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    println!("📦 Scanned {} verified user apps at startup.", apps.len());
    apps
}

fn launch(a: &App) {
    let _ = Command::new("explorer.exe")
        .arg(&a.id)
        .spawn();
}

static LAST_YOUTUBE_VIDEOS: Mutex<Vec<String>> = Mutex::new(Vec::new());

const BROWSER_STEMS: &[&str] = &[
    "chrome", "msedge", "brave", "firefox", "opera", "vivaldi", "thorium", "arc", "waterfox",
];

unsafe fn process_image_path_for_pid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if h.is_null() {
        return None;
    }
    let mut pbuf = [0u16; 1024];
    let mut sz = pbuf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, 0, pbuf.as_mut_ptr(), &mut sz);
    CloseHandle(h);
    if ok != 0 && sz > 0 {
        Some(String::from_utf16_lossy(&pbuf[..sz as usize]))
    } else {
        None
    }
}

fn foreground_browser_exe() -> Option<(String, String)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, &mut pid);
        let full = process_image_path_for_pid(pid)?;
        let stem = std::path::Path::new(&full)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(norm)
            .unwrap_or_default();
        if BROWSER_STEMS.iter().any(|&b| stem == b) {
            Some((stem, full))
        } else {
            None
        }
    }
}

fn find_running_browser_exe(target_stem: Option<&str>) -> Option<(String, String)> {
    for w in list_windows() {
        let exe_stem = norm(&w.exe);
        let matched = match target_stem {
            Some(want) => exe_stem == want,
            None => BROWSER_STEMS.iter().any(|&b| exe_stem == b),
        };
        if matched {
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(w.hwnd, &mut pid);
                if let Some(full) = process_image_path_for_pid(pid) {
                    return Some((exe_stem, full));
                }
            }
        }
    }
    None
}

fn open_url_in_target_browser(service: &str, url: &str) {
    let exe_pair = match service {
        "active_browser" => foreground_browser_exe().or_else(|| find_running_browser_exe(None)),
        "chrome" | "msedge" | "brave" | "firefox" | "opera" => {
            find_running_browser_exe(Some(service))
        }
        _ => None,
    };

    if let Some((stem, full_path)) = exe_pair {
        let mut cmd = Command::new(&full_path);
        if stem == "firefox" || stem == "waterfox" {
            cmd.arg("--new-tab");
        }
        if cmd.arg(url).spawn().is_ok() {
            return;
        }
    }

    let _ = Command::new("explorer.exe").arg(url).spawn();
}

fn contains_word_phrase(haystack: &str, phrase: &str) -> bool {
    let h = format!(" {haystack} ");
    let p = format!(" {phrase} ");
    h.contains(&p)
}

pub fn parse_search_intent_with_state(
    text: &str,
    yt_open: bool,
    sp_open: bool,
    yt_session_active: bool,
) -> Option<(String, String)> {
    let norm_text = norm(text);

    let is_search = norm_text.starts_with("search")
        || norm_text.contains(" search ")
        || norm_text.ends_with(" search")
        || norm_text.contains("dhundo")
        || norm_text.contains("khojo");

    if !is_search {
        return None;
    }

    let explicit_this_browser = contains_word_phrase(&norm_text, "this browser")
        || contains_word_phrase(&norm_text, "ei browser")
        || contains_word_phrase(&norm_text, "current browser")
        || contains_word_phrase(&norm_text, "active browser")
        || contains_word_phrase(&norm_text, "browser");

    let service = if norm_text.contains("spotify") {
        "spotify".to_string()
    } else if norm_text.contains("youtube") {
        "youtube".to_string()
    } else if explicit_this_browser {
        "active_browser".to_string()
    } else if contains_word_phrase(&norm_text, "chrome") {
        "chrome".to_string()
    } else if contains_word_phrase(&norm_text, "edge") {
        "msedge".to_string()
    } else if contains_word_phrase(&norm_text, "brave") {
        "brave".to_string()
    } else if contains_word_phrase(&norm_text, "firefox") {
        "firefox".to_string()
    } else if contains_word_phrase(&norm_text, "opera") {
        "opera".to_string()
    } else if norm_text.contains("google") {
        "google".to_string()
    } else if yt_open && !sp_open {
        "youtube".to_string()
    } else if sp_open && !yt_open {
        "spotify".to_string()
    } else if yt_open && sp_open {
        if yt_session_active {
            "youtube".to_string()
        } else {
            "spotify".to_string()
        }
    } else {
        // Neither YouTube nor Spotify is running -> default to Browser Google search!
        "google".to_string()
    };

    let clean = strip_phrases(&norm_text, &[
        "search",
        "dhundo",
        "khojo",
        "on youtube",
        "in youtube",
        "youtube mein",
        "youtube me",
        "youtube par",
        "youtube",
        "on spotify",
        "in spotify",
        "spotify mein",
        "spotify me",
        "spotify par",
        "spotify",
        "on google",
        "in google",
        "google mein",
        "google me",
        "google par",
        "google",
        "in this browser",
        "on this browser",
        "this browser",
        "ei browser a",
        "ei browser e",
        "ei browser er",
        "ei browser",
        "current browser",
        "active browser",
        "in browser",
        "on browser",
        "browser mein",
        "browser me",
        "browser par",
        "browser a",
        "browser e",
        "browser",
        "in chrome",
        "on chrome",
        "chrome a",
        "chrome e",
        "chrome",
        "in edge",
        "on edge",
        "edge a",
        "edge e",
        "edge",
        "in brave",
        "on brave",
        "brave",
        "in firefox",
        "on firefox",
        "firefox",
        "in opera",
        "on opera",
        "opera",
        "karo",
        "koro",
        "kardo",
        "kor",
        "kore",
        "dao",
        "please",
        "for",
        "about",
    ]);

    let clean_query = clean.split_whitespace().collect::<Vec<_>>().join(" ");

    Some((service, clean_query))
}

// Parse search intent cleanly regardless of word order
fn parse_search_intent(text: &str) -> Option<(String, String)> {
    let yt_open = is_app_open("youtube");
    let sp_open = is_app_open("spotify");
    let yt_session_active = media_native::has_session(YT_SESSION);
    parse_search_intent_with_state(text, yt_open, sp_open, yt_session_active)
}

// ---------- window management (win32) ----------

struct Win {
    hwnd: HWND,

    title: String,

    exe: String,
}

unsafe extern "system" fn enum_win(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam as *mut Vec<Win>);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

    if ex & WS_EX_TOOLWINDOW != 0 {
        return 1;
    }

    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }

    let len = GetWindowTextLengthW(hwnd);

    if len <= 0 {
        return 1;
    }

    let mut buf = vec![0u16; (len + 1) as usize];

    GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);

    let title = String::from_utf16_lossy(&buf[..len as usize]);

    let title = title.trim();

    if title.is_empty() {
        return 1;
    }

    let mut pid = 0u32;

    GetWindowThreadProcessId(hwnd, &mut pid);

    let mut exe = String::new();

    if pid != 0 {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);

        if !h.is_null() {
            let mut pbuf = [0u16; 1024];

            let mut sz = pbuf.len() as u32;

            if QueryFullProcessImageNameW(h, 0, pbuf.as_mut_ptr(), &mut sz) != 0 {
                let full = String::from_utf16_lossy(&pbuf[..sz as usize]);

                exe = std::path::Path::new(&full)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
            }

            CloseHandle(h);
        }
    }

    list.push(Win {
        hwnd,

        title: title.to_string(),

        exe,
    });

    1
}

fn list_windows() -> Vec<Win> {
    let mut list = Vec::new();

    unsafe {
        EnumWindows(Some(enum_win), &mut list as *mut _ as LPARAM);
    }

    list
}

pub fn is_process_running(name: &str) -> bool {
    let q = norm(name);

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);

        if !snap.is_null() && snap != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            let mut pe: PROCESSENTRY32W = std::mem::zeroed();

            pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snap, &mut pe) != 0 {
                loop {
                    let len = pe
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(pe.szExeFile.len());

                    let exe = String::from_utf16_lossy(&pe.szExeFile[..len]);

                    if norm(&exe).contains(&q) {
                        CloseHandle(snap);

                        return true;
                    }

                    if Process32NextW(snap, &mut pe) == 0 {
                        break;
                    }
                }
            }

            CloseHandle(snap);
        }
    }

    false
}

fn is_app_open(name: &str) -> bool {
    let q = norm(name);
    let wins = list_windows();

    // 1. Check open top-level windows first
    if wins.iter().any(|w| {
        let exe = norm(&w.exe);
        let title = norm(&w.title);
        exe == q || exe.contains(&q) || title.contains(&q)
    }) {
        return true;
    }

    // 2. Check native GSMTC media sessions
    let session_name: &str = if q == "youtube" { YT_SESSION } else { &q };
    if media_native::has_session(session_name) {
        return true;
    }

    // 3. For apps other than Spotify, check if process is running
    if q != "spotify" && is_process_running(&q) {
        return true;
    }

    false
}

fn window_action(action: &str, i: usize, wins: &[Win]) {
    let w = &wins[i];

    unsafe {
        match action {
            "close" => {
                PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
            }

            "minimize" => {
                PostMessageW(w.hwnd, WM_SYSCOMMAND, SC_MINIMIZE, 0);
                ShowWindow(w.hwnd, SW_MINIMIZE);
            }

            "maximize" => {
                ShowWindow(w.hwnd, SW_MAXIMIZE);

                SetForegroundWindow(w.hwnd);
            }

            "focus" => {
                if IsIconic(w.hwnd) != 0 {
                    ShowWindow(w.hwnd, SW_RESTORE);
                } else {
                    ShowWindow(w.hwnd, SW_RESTORE);
                }

                SetForegroundWindow(w.hwnd);
            }

            _ => {}
        }
    }
}

// ---------- smart resolver with auto-pick and voice choice ----------

fn resolve<F>(
    target: &str,

    total: usize,

    name_of: &F,

    score_of: &impl Fn(&str, usize) -> f64,

    auto_thresh: f64,

    aliases: &mut HashMap<String, String>,

    voice_inst: Option<&voice::Voice>,
) -> Option<usize>
where
    F: Fn(usize) -> String,
{
    let norm_target = norm(target);

    if let Some(saved) = aliases.get(&norm_target) {
        if let Some((i, s)) = pick(total, |i| score(saved, &name_of(i))) {
            if s >= 0.70 {
                return Some(i);
            }
        }
    }

    let mut ranked: Vec<(usize, f64)> = (0..total)
        .map(|i| (i, score_of(target, i)))
        .filter(|&(_, s)| s >= 0.35)
        .collect();

    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if ranked.is_empty() {
        println!("I could not find anything matching \"{target}\".");

        return None;
    }

    let best = ranked[0];

    let second = ranked.get(1).copied().unwrap_or((0, 0.0));

    // 1. Strong match: auto-pick the best one immediately without asking!

    if best.1 >= auto_thresh {
        return Some(best.0);
    }

    // 2. If the top 2 matches have the exact same name/exe, it's the SAME app! Auto-pick!

    if name_of(best.0).eq_ignore_ascii_case(&name_of(second.0)) {
        return Some(best.0);
    }

    // 3. Clear winner: if best is significantly ahead of second, auto-pick!

    if (best.1 - second.1) >= 0.06 && best.1 >= 0.65 {
        return Some(best.0);
    }

    // 4. If only 1 candidate found, auto-pick!

    let cand_count = ranked.len().min(5);

    if cand_count == 1 {
        return Some(best.0);
    }

    // 5. Truly ambiguous: show options and support VOICE choice!

    println!("\nI found {cand_count} possible matches for \"{target}\":");

    for (k, &(idx, s)) in ranked.iter().take(cand_count).enumerate() {
        println!("  {}) {} ({:.0}%)", k + 1, name_of(idx), s * 100.0);
    }

    let mut choice_idx = None;

    if let Some(v) = voice_inst {
        println!("🎤 Say 'number 1', 'number 2', or the name (or press Enter for #1):");

        print!("\x07"); // System alert chime

        io::stdout().flush().ok();

        if let Ok(Some(heard)) = v.wait_for_utterance() {
            let heard_text = norm(&heard.text);

            println!("🗣️ Heard choice: \"{}\"", heard.text);

            if heard_text.contains("one")
                || heard_text.contains("1")
                || heard_text.contains("first")
                || heard_text.contains("number one")
            {
                choice_idx = Some(0);
            } else if heard_text.contains("two")
                || heard_text.contains("2")
                || heard_text.contains("second")
                || heard_text.contains("number two")
            {
                if cand_count >= 2 {
                    choice_idx = Some(1);
                }
            } else if heard_text.contains("three")
                || heard_text.contains("3")
                || heard_text.contains("third")
                || heard_text.contains("number three")
            {
                if cand_count >= 3 {
                    choice_idx = Some(2);
                }
            } else if heard_text.contains("four") || heard_text.contains("4") {
                if cand_count >= 4 {
                    choice_idx = Some(3);
                }
            } else if heard_text.contains("five") || heard_text.contains("5") {
                if cand_count >= 5 {
                    choice_idx = Some(4);
                }
            } else {
                if let Some((idx, s)) =
                    pick(cand_count, |k| score(&heard_text, &name_of(ranked[k].0)))
                {
                    if s >= 0.55 {
                        choice_idx = Some(idx);
                    }
                }
            }
        }
    }

    let choice = match choice_idx {
        Some(c) => c,

        None => {
            let reply = ask("Which one? [1-5, Enter choice or 0 to cancel]: ");

            let reply = reply.trim();

            if reply.is_empty() || reply == "0" {
                println!("Cancelled (no selection made).");

                return None;
            } else {
                match reply.parse::<usize>() {
                    Ok(c) if c >= 1 && c <= cand_count => c - 1,

                    _ => {
                        println!("Cancelled (invalid selection).");

                        return None;
                    }
                }
            }
        }
    };

    let picked_idx = ranked[choice].0;

    // Only save to aliases if user explicitly picked by voice or console to avoid poisoning

    if choice_idx.is_some() {
        let picked_name = name_of(picked_idx);

        aliases.insert(norm_target, picked_name);

        save_aliases(aliases);
    }

    Some(picked_idx)
}

// ---------- extract command from spoken text ----------

#[allow(dead_code)]
fn is_only_wake_words(text: &str) -> bool {
    let mut rest = norm(text);

    for &ww in WAKE_WORDS {
        rest = rest.replace(ww, " ");
    }

    rest.split_whitespace().next().is_none()
}

fn get_active_wake_words(chosen: &str) -> Vec<&'static str> {
    match chosen.to_lowercase().as_str() {
        "jarvis" | "hey jarvis" => vec![
            "hey jarvis",
            "ok jarvis",
            "okay jarvis",
            "hi jarvis",
            "a jarvis",
            "jarvis",
            "hey jarvish",
            "ok jarvish",
            "jarvish",
            "hey jarves",
            "jarves",
            "hey jervis",
            "jervis",
            "hey jarviz",
            "jarviz",
            "hey darvis",
            "darvis",
            "hey charvis",
            "charvis",
            "hey harvis",
            "harvis",
            "hey travis",
            "ok travis",
            "travis",
            "hey service",
            "service",
            "hey java",
            "java",
            "hey job is",
            "job is",
            "hey harvest",
            "harvest",
            "hey jar vis",
            "jar vis",
            "hey jar was",
            "jar was",
            "jharvis",
            "tarvis",
        ],
        "pc" | "hey pc" | "computer" | "hey computer" => vec![
            "hey pc",
            "ok pc",
            "okay pc",
            "hi pc",
            "a pc",
            "pc",
            "hey p c",
            "ok p c",
            "okay p c",
            "hi p c",
            "p c",
            "hey computer",
            "ok computer",
            "okay computer",
            "hi computer",
            "computer",
            "hey bc",
            "ok bc",
            "hey b c",
            "b c",
            "apc",
            "hey apc",
            "hey pcs",
            "pcs",
        ],
        "alexa" | "hey alexa" => vec![
            "hey alexa",
            "ok alexa",
            "okay alexa",
            "hi alexa",
            "a alexa",
            "alexa",
            "hey alaxa",
            "alaxa",
            "hey elixa",
            "elixa",
            "hey alexia",
            "alexia",
            "hey alexis",
            "alexis",
            "hey aleksa",
            "aleksa",
            "hey alex",
            "ok alex",
            "alex",
            "hey lexa",
            "lexa",
            "a lex a",
        ],
        "inex" | "hey inex" => vec![
            "hey inex",
            "ok inex",
            "inex",
            "hey lenex",
            "ok lenex",
            "lenex",
            "hey linux",
            "ok linux",
            "linux",
            "hey lynx",
            "lynx",
            "hey lenox",
            "lenox",
            "hey index",
            "ok index",
            "index",
            "hey in x",
            "in x",
            "hey annex",
            "annex",
            "hey innex",
            "innex",
            "hey nx",
            "nx",
        ],
        _ => vec![
            "hey google",
            "ok google",
            "okay google",
            "google",
            "hi google",
            "hai google",
            "k google",
            "a google",
            "hey googel",
            "o google",
        ],
    }
}

fn collapse_pc_tokens(s: &str) -> String {
    let mut padded = format!(" {s} ");
    padded = padded.replace(" p c ", " pc ").replace(" b c ", " bc ");
    padded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_word_phrase(haystack: &str, phrase: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    while let Some(pos) = haystack[offset..].find(phrase) {
        let start = offset + pos;
        let end = start + phrase.len();
        let left_ok = start == 0 || haystack.as_bytes()[start - 1] == b' ';
        let right_ok = end == haystack.len() || haystack.as_bytes()[end] == b' ';
        if left_ok && right_ok {
            return Some((start, end));
        }
        offset = start + phrase.len().max(1);
    }
    None
}

fn extract_command(
    raw_text: &str,
    require_wake_word: bool,
    chosen_wake_word: &str,
) -> Option<String> {
    let lower = raw_text.to_lowercase();
    let cleaned = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>();
    let mut norm_text = collapse_pc_tokens(&cleaned);

    if !require_wake_word {
        return Some(norm_text);
    }

    let active_words = get_active_wake_words(chosen_wake_word);
    let mut matched_wake_word = false;

    // 1. Exact or prefix word match against any active wake word or phonetic variant
    for &ww in &active_words {
        if norm_text == ww {
            norm_text.clear();
            matched_wake_word = true;
            break;
        }
        if let Some((start, end)) = find_word_phrase(&norm_text, ww) {
            if start == 0 {
                norm_text = norm_text[end..].trim().to_string();
                matched_wake_word = true;
                break;
            }
        }
    }

    // 2. Check if wake word appears anywhere on word boundaries in the sentence
    if !matched_wake_word {
        for &ww in &active_words {
            if let Some((_, end)) = find_word_phrase(&norm_text, ww) {
                norm_text = norm_text[end..].trim().to_string();
                matched_wake_word = true;
                break;
            }
        }
    }

    // 3. Fuzzy similarity on initial 1 or 2 words (slight STT distortion)
    if !matched_wake_word {
        let words: Vec<&str> = norm_text.split_whitespace().collect();
        if !words.is_empty() {
            let primary_key = match chosen_wake_word.to_lowercase().as_str() {
                "jarvis" | "hey jarvis" => "jarvis",
                "alexa" | "hey alexa" => "alexa",
                "inex" | "hey inex" => "inex",
                "pc" | "hey pc" | "computer" | "hey computer" => "pc",
                _ => "google",
            };

            // Only run partial-similarity fuzzy matching on words >= 4 chars (2-char "pc" uses active_words above)
            if primary_key.len() >= 4 {
                let sc0 = strsim::jaro_winkler(words[0], primary_key);
                if sc0 >= 0.78 {
                    matched_wake_word = true;
                    norm_text = words[1..].join(" ");
                } else if words.len() >= 2 {
                    let prefix_greet = matches!(words[0], "hey" | "ok" | "okay" | "hi" | "a" | "o");
                    let sc1 = strsim::jaro_winkler(words[1], primary_key);
                    let w2 = format!("{} {}", words[0], words[1]);
                    let target2 = format!("hey {}", primary_key);
                    let sc2 = strsim::jaro_winkler(&w2, &target2);
                    if (prefix_greet && sc1 >= 0.78) || sc2 >= 0.80 {
                        matched_wake_word = true;
                        norm_text = words[2..].join(" ");
                    }
                }
            }
        }
    }

    if matched_wake_word {
        loop {
            let mut stripped = false;
            for &ww in &active_words {
                if norm_text == ww {
                    norm_text.clear();
                    stripped = true;
                    break;
                }
                if let Some((start, end)) = find_word_phrase(&norm_text, ww) {
                    if start == 0 {
                        norm_text = norm_text[end..].trim().to_string();
                        stripped = true;
                        break;
                    }
                }
            }
            if !stripped {
                break;
            }
        }

        if norm_text.is_empty() {
            return Some(String::new()); // Wake word only
        }

        return Some(norm_text);
    }

    None
}

fn kill_processes_by_name(exe_name: &str) -> bool {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let target = if exe_name.ends_with(".exe") {
        exe_name.to_string()
    } else {
        format!("{}.exe", exe_name)
    };

    let status = Command::new("taskkill")
        .args(["/F", "/T", "/IM", &target])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    match status {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

fn close_app(name: &str) -> bool {
    let q = norm(name);
    if q.is_empty() {
        return false;
    }

    // Critical protection: NEVER close system core components
    let protected = [
        "explorer", "windows explorer", "file explorer", "file manager",
        "nexus", "aura", "antigravity", "island", "inex island", "inex-island",
        "system", "svchost", "dwm", "csrss", "lsass", "winlogon", "services", "taskhostw",
    ];
    if protected.iter().any(|&p| q == p) {
        if q.contains("explorer") || q.contains("file") {
            let wins = list_windows();
            let mut count = 0;
            for w in &wins {
                if norm(&w.exe) == "explorer" {
                    unsafe {
                        PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
                    }
                    count += 1;
                }
            }
            println!("🪟 Closed {} File Explorer window(s)", count);
            return true;
        }
        println!("⚠️ Protected process, refusal to close: {}", q);
        return false;
    }

    if q == "youtube" {
        close_app_by_name("youtube");
        return true;
    }

    let wins = list_windows();
    let mut closed_count = 0;

    for w in &wins {
        let exe_norm = norm(&w.exe);
        let title_norm = norm(&w.title);

        if q == "chrome" {
            if title_norm.contains("youtube") {
                continue;
            }
            if exe_norm.contains("chrome") {
                unsafe {
                    PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
                }
                closed_count += 1;
            }
        } else if exe_norm == q || exe_norm.contains(&q) || title_norm.contains(&q) {
            unsafe {
                PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
            }
            closed_count += 1;
        }
    }

    // Process termination for apps (vital for Discord, Spotify, and background apps)
    let is_proc_running = is_process_running(&q);
    let mut killed = false;

    let mut targets_to_kill = vec![q.clone()];
    if q == "discord" {
        targets_to_kill.push("Discord".to_string());
    } else if q == "spotify" {
        targets_to_kill.push("Spotify".to_string());
    } else if q == "vscode" || q == "code" || q == "vs code" {
        targets_to_kill.push("Code".to_string());
    }

    if is_proc_running || closed_count > 0 || q == "discord" || q == "spotify" {
        for t in &targets_to_kill {
            if kill_processes_by_name(t) {
                killed = true;
            }
        }
    }

    if closed_count > 0 || killed || is_proc_running {
        println!("🛑 Closed app: {} (windows: {}, killed: {})", q, closed_count, killed);
        true
    } else {
        println!("ℹ️ App \"{}\" is already closed or not running.", q);
        true
    }
}

fn minimize_app(target: &str) -> bool {
    let q = norm(target);
    if q.is_empty() {
        return false;
    }

    println!("🪟 Minimizing app: {}", q);

    let wins = list_windows();
    let mut count = 0;

    for w in &wins {
        let w_exe = norm(&w.exe);
        let w_title = norm(&w.title);

        if w_exe == q || w_exe.contains(&q) || w_title.contains(&q) {
            unsafe {
                PostMessageW(w.hwnd, WM_SYSCOMMAND, SC_MINIMIZE, 0);
                ShowWindow(w.hwnd, SW_MINIMIZE);
            }
            count += 1;
        }
    }

    if count > 0 {
        println!("✅ Minimized {} window(s) of {}", count, q);
        return true;
    }

    // Fuzzy match on open windows
    let mut best_sc = 0.0;
    let mut best_hwnd: HWND = std::ptr::null_mut();
    for w in &wins {
        let sc = score(&q, &w.exe).max(0.9 * score(&q, &w.title));
        if sc > best_sc {
            best_sc = sc;
            best_hwnd = w.hwnd;
        }
    }

    if best_sc >= 0.65 && !best_hwnd.is_null() {
        unsafe {
            PostMessageW(best_hwnd, WM_SYSCOMMAND, SC_MINIMIZE, 0);
            ShowWindow(best_hwnd, SW_MINIMIZE);
        }
        println!("✅ Minimized closest matching window (score {:.2})", best_sc);
        return true;
    }

    println!("ℹ️ App \"{}\" is not open or already minimized.", q);
    true
}

fn close_app_by_name(name: &str) {
    let wins = list_windows();

    let q = norm(name);

    let mut count = 0;

    for w in &wins {
        let exe_norm = norm(&w.exe);

        let title_norm = norm(&w.title);

        if q == "chrome" {
            // When user says "close chrome", DO NOT close YouTube PWA or YouTube tabs!

            if title_norm.contains("youtube") {
                continue;
            }

            if exe_norm.contains("chrome") {
                unsafe {
                    PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
                }

                count += 1;
            }
        } else if q == "youtube" {
            // When user says "close youtube", close only YouTube windows

            if title_norm.contains("youtube") || exe_norm.contains("youtube") {
                unsafe {
                    PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
                }

                count += 1;
            }
        } else {
            if exe_norm.contains(&q) || title_norm.contains(&q) {
                unsafe {
                    PostMessageW(w.hwnd, WM_CLOSE, 0, 0);
                }

                count += 1;
            }
        }
    }

    println!("🪟 Closed {} window(s) of: {}", count, name);
}

fn launch_app_smart(a: &App) {
    println!("🚀 Launching: {}", a.name);
    launch(a);
}

// ---------- execute single command ----------

// Skip intent, whatever the filler words are:
// "next", "play next", "next video", "play the next video please", "previous song", "skip" ...
// Some(true) = next, Some(false) = previous, None = not a skip command
// ("play next level song" is NOT a skip command because "level" is not a filler word).
fn media_nav_intent(lnorm: &str) -> Option<bool> {
    const NEXT: &[&str] = &["next", "skip", "agla", "agli"];
    const PREV: &[&str] = &["previous", "prev", "pichla", "pichli"];
    const FILLER: &[&str] = &[
        "play", "the", "a", "an", "to", "this", "one", "video", "vedio", "song", "gana", "gaan",
        "track", "music", "please", "now", "youtube", "spotify", "on", "in", "koro", "karo",
        "kardo", "kar", "do", "me", "ta",
    ];
    let words: Vec<&str> = lnorm.split_whitespace().collect();
    let has_next = words.iter().any(|w| NEXT.contains(w));
    let has_prev = words.iter().any(|w| PREV.contains(w));
    if has_next == has_prev {
        return None;
    }
    let only_known = words
        .iter()
        .all(|w| NEXT.contains(w) || PREV.contains(w) || FILLER.contains(w));
    if !only_known {
        return None;
    }
    Some(has_next)
}

fn execute_single_command(
    raw_cmd: &str,

    vocab: &Vocab,

    apps: &mut Vec<App>,

    aliases: &mut HashMap<String, String>,

    voice_inst: Option<&voice::Voice>,
) -> bool {
    let line = raw_cmd.trim();

    if line.is_empty() {
        return false;
    }

    let mut lnorm = norm(line);

    // Clean up trailing / embedded wake words: "focus Chrome hey Google" -> "focus chrome"

    lnorm = lnorm
        .replace("hey google", " ")
        .replace("ok google", " ")
        .replace("okay google", " ")
        .replace("okey google", " ")
        .replace("hey inex", " ")
        .replace("ok inex", " ")
        .replace("hey aura", " ")
        .replace("ok aura", " ")
        .replace("okay aura", " ")
        .replace("okey aura", " ")
        .replace("hey lenex", " ")
        .replace("ok lenex", " ")
        .replace("hey linux", " ")
        .replace("ok linux", " ")
        .replace("hey jarvis", " ")
        .replace("ok jarvis", " ")
        .replace("hey alexa", " ")
        .replace("ok alexa", " ")
        .replace("hey pc", " ")
        .replace("ok pc", " ")
        .replace("hey p c", " ")
        .replace("ok p c", " ")
        .replace("hey computer", " ")
        .replace("lenex", "inex")
        .replace("index", "inex")
        .replace("spotyfi", "spotify")
        .replace("spottify", "spotify")
        .replace("spotfy", "spotify")
        .replace("discards", "discord")
        .replace("discard", "discord")
        .replace("discort", "discord")
        .replace("puse", "pause")
        .replace("prevuse", "previous")
        .replace("privious", "previous");

    let mut words: Vec<&str> = lnorm.split_whitespace().collect();
    if let Some(&first) = words.first() {
        if first == "okey" || first == "okay" || first == "ok" {
            words.remove(0);
            lnorm = words.join(" ");
        }
    }

    // Handle speech misrecognitions (e.g. "plato ka toka" -> "play toka toka")
    if lnorm.starts_with("plato ") {
        lnorm = format!("play to{}", &lnorm[5..]);
    } else if lnorm.starts_with("ple ") {
        lnorm = format!("play{}", &lnorm[3..]);
    } else if lnorm.starts_with("played ") {
        lnorm = format!("play{}", &lnorm[6..]);
    }

    // Normalize action verb variations (minimise / minimises -> minimize, etc.)
    if lnorm.contains("minimise") {
        lnorm = lnorm
            .replace("minimises", "minimize")
            .replace("minimised", "minimize")
            .replace("minimising", "minimize")
            .replace("minimise", "minimize");
    }
    if lnorm.contains("minimizing") {
        lnorm = lnorm.replace("minimizing", "minimize");
    }

    if lnorm.contains("maximise") {
        lnorm = lnorm
            .replace("maximises", "maximize")
            .replace("maximised", "maximize")
            .replace("maximising", "maximize")
            .replace("maximise", "maximize");
    }
    if lnorm.contains("maximizing") {
        lnorm = lnorm.replace("maximizing", "maximize");
    }

    if lnorm.contains("focuses") || lnorm.contains("focused") {
        lnorm = lnorm
            .replace("focuses", "focus")
            .replace("focused", "focus");
    }

    if lnorm.contains("closes") || lnorm.contains("closed") {
        lnorm = lnorm.replace("closes", "close").replace("closed", "close");
    }

    lnorm = lnorm.split_whitespace().collect::<Vec<_>>().join(" ");

    // 1. System Power & Security Commands
    if lnorm == "lock" || lnorm == "lock pc" || lnorm == "lock computer" || lnorm == "lock screen" {
        println!("🔒 Locking PC Workstation...");
        unsafe {
            windows_sys::Win32::System::Shutdown::LockWorkStation();
        }
        return true;
    }

    if lnorm == "shutdown"
        || lnorm == "shutdown pc"
        || lnorm == "turn off pc"
        || lnorm == "shut down pc"
    {
        println!("🛑 Shutting down PC in 15 seconds (Say 'cancel shutdown' to abort)...");
        let _ = Command::new("shutdown").args(["/s", "/t", "15"]).spawn();
        return true;
    }

    if lnorm == "restart" || lnorm == "restart pc" || lnorm == "reboot pc" || lnorm == "reboot" {
        println!("🔄 Restarting PC in 15 seconds (Say 'cancel shutdown' to abort)...");
        let _ = Command::new("shutdown").args(["/r", "/t", "15"]).spawn();
        return true;
    }

    if lnorm == "cancel shutdown" || lnorm == "abort shutdown" || lnorm == "stop shutdown" {
        println!("✅ Shutdown aborted!");
        let _ = Command::new("shutdown").args(["/a"]).spawn();
        return true;
    }

    // 1b. Native Circle to Search Overlay Toggle ("on circle to search" / "off circle to search")
    match modules::circle_search::handle_voice_command(&lnorm) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            eprintln!("Circle to Search voice command failed: {e}");
            return false;
        }
    }

    // 2. Volume Commands (Per-App Mixer Control & Master PC Volume)
    if handle_volume_command(&lnorm) {
        return true;
    }

    // 3. Scrolling Commands ("halka scroll", "scroll down", "scroll up")
    if lnorm.contains("scroll down")
        || lnorm == "scroll"
        || lnorm == "page down"
        || lnorm.contains("scroll niche")
        || lnorm.contains("halka scroll")
    {
        println!("📜 Scrolling down...");
        scroll(-240);
        return true;
    }

    if lnorm.contains("scroll up") || lnorm == "page up" || lnorm.contains("scroll upor") {
        println!("📜 Scrolling up...");
        scroll(240);
        return true;
    }

    // 3b. Window Minimize Commands ("minimize spotify", "minimise spotyfi", "spotify minimize")
    let is_minimize_intent = lnorm.starts_with("minimize ")
        || lnorm.ends_with(" minimize")
        || lnorm.contains(" minimize ")
        || lnorm.contains("choto koro")
        || lnorm.contains("hide ")
        || lnorm.ends_with(" hide")
        || lnorm == "minimize";

    if is_minimize_intent {
        let mut target = lnorm.clone();
        for w in &["minimize", "choto koro", "choto", "koro", "karo", "the", "app", "window", "hide", "please"] {
            target = target.replace(w, " ");
        }
        let target = target.split_whitespace().collect::<Vec<_>>().join(" ");
        if target.is_empty() {
            unsafe {
                let fg = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
                if !fg.is_null() {
                    PostMessageW(fg, WM_SYSCOMMAND, SC_MINIMIZE, 0);
                    ShowWindow(fg, SW_MINIMIZE);
                    println!("🪟 Minimized active window.");
                    return true;
                }
            }
            return true;
        } else {
            return minimize_app(&target);
        }
    }

    // 3c. Window / App Close Commands ("close discord", "close spotify", "discord close", "kete dao")
    let is_close_intent = lnorm.starts_with("close ")
        || lnorm.ends_with(" close")
        || lnorm.contains(" close ")
        || lnorm.starts_with("quit ")
        || lnorm.ends_with(" quit")
        || lnorm.starts_with("exit ")
        || lnorm.ends_with(" exit")
        || lnorm.contains(" bondho koro")
        || lnorm.contains(" band karo")
        || lnorm.contains(" kete dao")
        || lnorm.contains(" cut karo")
        || lnorm.contains(" kato ");

    let is_media_stop = lnorm.contains("music")
        || lnorm.contains("song")
        || lnorm.contains("gana")
        || lnorm.contains("gaan")
        || lnorm.contains("video");

    if is_close_intent && !is_media_stop {
        let mut target = lnorm.clone();
        for w in &[
            "close", "quit", "exit", "bondho koro", "band karo", "kete dao",
            "cut karo", "kato", "koro", "karo", "the", "app", "window", "please"
        ] {
            target = target.replace(w, " ");
        }
        let target = target.split_whitespace().collect::<Vec<_>>().join(" ");
        if !target.is_empty() {
            return close_app(&target);
        }
    }

    // 4. Select Video Search Results (play 1st, 2nd, or 3rd video)
    if lnorm.contains("first video")
        || lnorm.contains("number one video")
        || lnorm.contains("first one")
        || lnorm.contains("fast one")
        || lnorm.contains("play fast")
        || lnorm.contains("play first")
        || lnorm == "play fast one"
        || lnorm == "play first one"
        || lnorm == "first"
        || lnorm == "fast"
        || lnorm == "1st"
        || lnorm == "number one"
        || lnorm == "number 1"
        || lnorm.contains("1st video")
        || lnorm.contains("fast video")
        || lnorm.contains("number 1 video")
    {
        if yt_in_window("pick", "1") {
            return true;
        }
        if let Ok(lock) = LAST_YOUTUBE_VIDEOS.lock() { if let Some(first) = lock.first() {
                let url = format!("https://www.youtube.com/watch?v={first}");
                println!("▶️ Auto-playing 1st video in YouTube App: {url}");
                open_youtube_app(apps, Some(&url));
                return true;
            }
        }

        println!("▶️ Auto-playing YouTube playback via native media controls...");
        media_native::control_media(media_native::MediaAction::Play, Some(YT_SESSION));
        return true;
    }

    if lnorm.contains("second video")
        || lnorm.contains("number two video")
        || lnorm.contains("second one")
        || lnorm.contains("play second")
        || lnorm == "play second one"
        || lnorm == "second"
        || lnorm == "2nd"
        || lnorm == "number two"
        || lnorm == "number 2"
        || lnorm.contains("2nd video")
        || lnorm.contains("number 2 video")
    {
        if yt_in_window("pick", "2") {
            return true;
        }
        if let Ok(lock) = LAST_YOUTUBE_VIDEOS.lock() { if lock.len() >= 2 {
                let second = &lock[1];
                let url = format!("https://www.youtube.com/watch?v={second}");
                println!("▶️ Auto-playing 2nd video in YouTube App: {url}");
                open_youtube_app(apps, Some(&url));
                return true;
            }
        }
        return true;
    }

    if lnorm.contains("third video")
        || lnorm.contains("number three video")
        || lnorm.contains("third one")
        || lnorm.contains("play third")
        || lnorm == "play third one"
        || lnorm == "third"
        || lnorm == "3rd"
        || lnorm == "number three"
        || lnorm == "number 3"
        || lnorm.contains("3rd video")
        || lnorm.contains("number 3 video")
    {
        if yt_in_window("pick", "3") {
            return true;
        }
        if let Ok(lock) = LAST_YOUTUBE_VIDEOS.lock() { if lock.len() >= 3 {
                let third = &lock[2];
                let url = format!("https://www.youtube.com/watch?v={third}");
                println!("▶️ Auto-playing 3rd video in YouTube App: {url}");
                open_youtube_app(apps, Some(&url));
                return true;
            }
        }
        return true;
    }

    // 6. Focus / Bring app to front

    if lnorm.starts_with("focus ")
        || lnorm.starts_with("show ")
        || lnorm.contains("samne lao")
        || lnorm.contains("to front")
    {
        let mut target_app = lnorm.clone();

        for w in &[
            "focus ",
            "show ",
            "bring ",
            "to front",
            "samne lao",
            "samne",
            "karo",
            "koro",
            "app",
            "the ",
        ] {
            target_app = target_app.replace(w, " ");
        }

        let q = norm(&target_app);

        let wins = list_windows();

        if let Some(w) = wins
            .iter()
            .find(|w| norm(&w.exe).contains(&q) || norm(&w.title).contains(&q))
        {
            println!("🪟 Bringing to front: {}", w.title);

            unsafe {
                if IsIconic(w.hwnd) != 0 {
                    ShowWindow(w.hwnd, SW_RESTORE);
                } else {
                    ShowWindow(w.hwnd, SW_SHOW);
                }

                SetForegroundWindow(w.hwnd);
            }

            return true;
        }
    }

    // 6b. Video Fullscreen Controls (YouTube / Video Player)

    let is_fullscreen_video = (lnorm.contains("full screen")
        || lnorm.contains("fullscreen")
        || lnorm.contains("full scren"))
        && (lnorm.contains("youtube")
            || lnorm.contains("video")
            || lnorm.contains("vedio")
            || lnorm == "full screen"
            || lnorm == "fullscreen");

    let is_exit_fullscreen = lnorm.contains("exit full screen")
        || lnorm.contains("exit fullscreen")
        || lnorm.contains("normal screen")
        || lnorm.contains("choto koro")
        || lnorm.contains("close fullscreen");

    if is_fullscreen_video && !lnorm.contains("maximize") {
        println!("🖥️ Fullscreen Video: Toggling YouTube / Video Fullscreen ('F')...");

        let wins = list_windows();

        if let Some(w) = wins
            .iter()
            .find(|w| norm(&w.title).contains("youtube") || norm(&w.exe).contains("youtube"))
        {
            unsafe {
                if IsIconic(w.hwnd) != 0 {
                    ShowWindow(w.hwnd, SW_RESTORE);
                } else {
                    ShowWindow(w.hwnd, SW_SHOW);
                }

                SetForegroundWindow(w.hwnd);
            }

            std::thread::sleep(Duration::from_millis(80));

            send_key(0x46); // 'F' toggles YouTube video fullscreen

            println!("✅ YouTube video fullscreen toggled ('F')");
        } else {
            send_key(0x46);

            println!("✅ Fullscreen key ('F') sent to active window");
        }

        return true;
    }

    if is_exit_fullscreen {
        println!("🖥️ Fullscreen Video: Exiting Fullscreen (Escape)...");

        let wins = list_windows();

        if let Some(w) = wins
            .iter()
            .find(|w| norm(&w.title).contains("youtube") || norm(&w.exe).contains("youtube"))
        {
            unsafe {
                SetForegroundWindow(w.hwnd);
            }

            std::thread::sleep(Duration::from_millis(60));
        }

        send_key(VK_ESCAPE as u8);

        return true;
    }

    // 7. Media Playback Controls (100% Native WinRT GSMTC - 0% Hotkeys)
    let youtube_open = is_app_open("youtube");
    let spotify_open = is_app_open("spotify");

    let get_media_target = |cmd: &str| -> Option<&'static str> {
        if cmd.contains("youtube") || cmd.contains("video") {
            Some(YT_SESSION)
        } else if cmd.contains("spotify") || cmd.contains("song") || cmd.contains("gana") || cmd.contains("gaan") {
            Some(SP_SESSION)
        } else {
            // nothing named: go by what is actually PLAYING first, then by what is open
            let yt_playing = media_native::is_playing(Some(YT_SESSION));
            let sp_playing = media_native::is_playing(Some(SP_SESSION));
            if yt_playing && !sp_playing {
                Some(YT_SESSION)
            } else if sp_playing && !yt_playing {
                Some(SP_SESSION)
            } else if youtube_open && !spotify_open {
                Some(YT_SESSION)
            } else if spotify_open && !youtube_open {
                Some(SP_SESSION)
            } else {
                None
            }
        }
    };

    // 7a. Previous track / video
    if media_nav_intent(&lnorm) == Some(false)
        || lnorm.contains("previous")
        || lnorm.contains("prev track")
        || lnorm.contains("pichla")
        || lnorm.contains("prevuse")
        || lnorm.contains("play previous")
        || lnorm.contains("prev video")
        || lnorm.contains("prev song")
    {
        let target = get_media_target(&lnorm);
        println!("⏮️ Media: Previous on {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_skip(false);
        } else if is_sp_target(target) {
            sp_previous();
        } else {
            media_native::control_media(media_native::MediaAction::Previous, target);
        }
        return true;
    }

    // 7b. Next track (supports "play next", "next video", "change the song")
    let is_next = media_nav_intent(&lnorm) == Some(true)
        || lnorm.contains("next song")
        || lnorm.contains("next track")
        || lnorm == "next"
        || lnorm == "play next"
        || lnorm == "next video"
        || lnorm.contains("skip")
        || lnorm.contains("change the song")
        || lnorm.contains("change song")
        || lnorm.contains("agla gana")
        || lnorm.contains("agla video");

    if is_next {
        let target = get_media_target(&lnorm);
        println!("⏭️ Media: Next on {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_skip(true);
        } else {
            media_native::control_media(media_native::MediaAction::Next, target);
        }
        return true;
    }

    // 7e. Pause / Stop
    let is_pause = lnorm.contains("pause")
        || lnorm.contains("stop music")
        || lnorm == "stop"
        || lnorm.contains("band karo")
        || lnorm.contains("ruk jao")
        || lnorm.contains("puse");

    if is_pause {
        let target = get_media_target(&lnorm);
        println!("⏸️ Media: Pausing {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_pause_resume(true);
        } else {
            media_native::control_media(media_native::MediaAction::Pause, target);
        }
        return true;
    }

    // 7f. Resume / Play
    if lnorm.contains("resume")
        || lnorm.contains("unpause")
        || lnorm == "play"
        || lnorm == "continue"
    {
        let target = get_media_target(&lnorm);
        println!("▶️ Media: Resuming {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_pause_resume(false);
        } else {
            media_native::control_media(media_native::MediaAction::Play, target);
        }
        return true;
    }

    // 8. Search commands: "search on YouTube travel with AK", "search on spotify...", "search funny cats..."

    if let Some((service, query)) = parse_search_intent(&lnorm) {
        let is_browser_target = !matches!(service.as_str(), "spotify" | "youtube");
        if query.is_empty() {
            println!("🔍 Opening {}...", service);

            if service == "spotify" {
                launch_spotify(None);
            } else if is_browser_target {
                open_url_in_target_browser(&service, "https://www.google.com/");
            } else {
                open_youtube(apps);
            }

            return true;
        }

        match service.as_str() {
            "spotify" => {
                launch_spotify(Some(&query));
                return true;
            }
            "youtube" => {
                search_youtube(apps, &query);
                return true;
            }
            _ => {
                let url = format!("https://www.google.com/search?q={}", url_encode(&query));
                println!("🌐 Searching in {}: {}", service, query);
                open_url_in_target_browser(&service, &url);
                return true;
            }
        }
    }

    // Compound commands: "open youtube and play [song / <name>]", "open spotify and play [song / <name>]"

    if (lnorm.contains("open") || lnorm.contains("kholo"))
        && (lnorm.contains("play")
            || lnorm.contains("song")
            || lnorm.contains("gana")
            || lnorm.contains("chalao")
            || lnorm.contains("chala")
            || lnorm.contains("bajao"))
    {
        let is_yt = lnorm.contains("youtube");

        let is_sp = lnorm.contains("spotify");

        if is_yt || is_sp {
            let clean = strip_phrases(&lnorm, &[
                "open",
                "kholo",
                "and",
                "aur",
                "ar",
                "please",
                "on youtube",
                "in youtube",
                "youtube mein",
                "youtube me",
                "youtube par",
                "youtube",
                "on spotify",
                "in spotify",
                "spotify mein",
                "spotify me",
                "spotify par",
                "spotify",
                "play",
                "the song",
                "song",
                "gana",
                "a song",
                "chalao",
                "chala",
                "bajao",
                "karo",
                "koro",
            ]);

            let clean_q = clean.split_whitespace().collect::<Vec<_>>().join(" ");

            let q_opt = if clean_q.is_empty() || clean_q == "music" || clean_q == "something" {
                None
            } else {
                Some(clean_q.as_str())
            };

            if is_yt {
                play_youtube(apps, q_opt);
            } else {
                launch_spotify(q_opt);
            }

            return true;
        }
    }

    // 9. Play commands: "play Believer", "play so pretty song on Spotify", "play song on YouTube", "song on Spotify", "play song"

    let is_play = (lnorm.starts_with("play")
        || lnorm.starts_with("song ")
        || lnorm.starts_with("gana ")
        || lnorm.ends_with(" play")
        || lnorm.contains("chalao")
        || lnorm.contains("chala")
        || lnorm.contains("bajao"))
        && lnorm != "play next";

    if is_play {
        let youtube_explicit = lnorm.contains("youtube");

        let spotify_explicit = lnorm.contains("spotify");

        let use_youtube = youtube_explicit || (youtube_open && !spotify_open && !spotify_explicit);

        let clean = strip_phrases(&lnorm, &[
            "play",
            "the song",
            "song",
            "gana",
            "a song",
            "ei gaan ta",
            "ei gan ta",
            "ei gaan",
            "ei gan",
            "gaan ta",
            "gan ta",
            "gaan",
            "on youtube",
            "in youtube",
            "youtube mein",
            "youtube me",
            "youtube par",
            "youtube",
            "on spotify",
            "in spotify",
            "spotify mein",
            "spotify me",
            "spotify par",
            "spotify",
            "karo",
            "koro",
            "kardo",
            "chalao",
            "chala",
            "bajao",
            "please",
            "dekhao",
            "ei",
            "ta",
            "eta",
            "ota",
        ]);

        let clean_query = clean.split_whitespace().collect::<Vec<_>>().join(" ");

        let query_opt = if clean_query.is_empty()
            || clean_query == "music"
            || clean_query == "something"
            || clean_query == "on"
        {
            None
        } else {
            Some(clean_query.as_str())
        };

        if use_youtube {
            play_youtube(apps, query_opt);
        } else {
            // Default to Spotify!

            launch_spotify(query_opt);
        }

        return true;
    }

    // 10. Quick Open / Close for YouTube & Spotify

    if (lnorm.contains("open") || lnorm.contains("kholo")) && lnorm.contains("youtube") {
        open_youtube(apps);

        return true;
    }

    if (lnorm.contains("open") || lnorm.contains("kholo")) && lnorm.contains("spotify") {
        open_spotify_app();

        return true;
    }

    if (lnorm.contains("close")
        || lnorm.contains("band")
        || lnorm.contains("cut")
        || lnorm.contains("kete"))
        && lnorm.contains("youtube")
    {
        close_app_by_name("youtube");

        return true;
    }

    if (lnorm.contains("close")
        || lnorm.contains("band")
        || lnorm.contains("cut")
        || lnorm.contains("kete"))
        && lnorm.contains("spotify")
    {
        close_app_by_name("spotify");

        return true;
    }

    // 10b. File Explorer & Folder Navigation (explicit folder/file requests only)

    if handle_file_explorer(&lnorm, voice_inst) {
        return true;
    }

    // 11. Standard App & Window Automation (Apps, Windows, Open, Close, Minimize, Maximize)

    if line.eq_ignore_ascii_case("apps") {
        for a in apps.iter().take(60) {
            println!("  {}", a.name);
        }

        println!("  ({} total)", apps.len());

        return true;
    }

    if line.eq_ignore_ascii_case("windows") {
        for w in list_windows() {
            println!("  [{}] {}", w.exe, w.title);
        }

        return true;
    }

    let Some(p) = parse(&lnorm, vocab) else {
        let single_norm = norm(&lnorm);

        if single_norm == "spotify" {
            open_spotify_app();
            return true;
        }

        if single_norm == "youtube" {
            open_youtube(apps);
            return true;
        }

        if single_norm == "chrome" {
            if let Some(a) = apps.iter().find(|a| norm(&a.name).contains("chrome")) {
                launch_app_smart(a);
            } else {
                let _ = Command::new("explorer.exe")
                    .arg("chrome.exe")
                    .spawn();
            }
            return true;
        }

        if single_norm == "discord" {
            if let Some(a) = apps.iter().find(|a| norm(&a.name) == "discord") {
                launch_app_smart(a);
            } else {
                let _ = Command::new("explorer.exe").arg("discord://").spawn();
            }
            return true;
        }

        if single_norm == "all song" || single_norm == "all songs" {
            launch_spotify(None);
            return true;
        }

        // Only exact app name match: NEVER randomly launch apps on unrecognized sentences!
        if !single_norm.is_empty() {
            if let Some(a) = apps.iter().find(|a| norm(&a.name) == single_norm) {
                println!("🚀 Launching app by exact name: {}", a.name);
                launch_app_smart(a);
                return true;
            }
        }

        println!("⚠️  Action not recognized in: \"{}\"", line);
        return false;
    };

    if p.target.is_empty() {
        println!("❓ What should I {}?", p.action);
        return false;
    }

    match p.action.as_str() {
        "open" => {
            let target_norm = norm(&p.target);

            // Special check for YouTube
            if target_norm == "youtube" || target_norm.starts_with("youtube ") {
                open_youtube(apps);
                return true;
            }

            // Special check for Spotify
            if target_norm == "spotify" {
                open_spotify_app();
                return true;
            }

            // Special check for Discord: exact app only, never fuzzy-matched to another app
            if target_norm == "discord" {
                if let Some(a) = apps.iter().find(|a| norm(&a.name) == "discord") {
                    launch_app_smart(a);
                } else {
                    let _ = Command::new("explorer.exe").arg("discord://").spawn();
                }
                return true;
            }

            let q = aliases
                .get(&target_norm)
                .cloned()
                .unwrap_or_else(|| p.target.clone());

            let label = |i: usize| apps[i].name.clone();
            let sc = |q: &str, i: usize| score(q, &apps[i].name);

            if let Some(i) = resolve(
                &p.target,
                apps.len(),
                &label,
                &sc,
                0.75,
                aliases,
                voice_inst,
            ) {
                if score(&q, &apps[i].name).max(score(&p.target, &apps[i].name)) < 0.70 {
                    println!("❌ \"{}\" app list-e nai (closest: {})", p.target, apps[i].name);
                    return false;
                }
                launch_app_smart(&apps[i]);
                return true;
            }
        }

        "close" => {
            return close_app(&p.target);
        }

        "minimize" => {
            return minimize_app(&p.target);
        }

        "maximize" | "focus" => {
            let wins = list_windows();

            if wins.is_empty() {
                println!("No open windows found.");
                return false;
            }

            let label = |i: usize| {
                if wins[i].exe.is_empty() {
                    wins[i].title.clone()
                } else {
                    wins[i].exe.clone()
                }
            };

            let sc = |q: &str, i: usize| score(q, &wins[i].exe).max(0.9 * score(q, &wins[i].title));

            if let Some(i) = resolve(
                &p.target,
                wins.len(),
                &label,
                &sc,
                0.68,
                aliases,
                voice_inst,
            ) {
                println!("🪟 Executing {} on: {}", p.action, label(i));
                window_action(&p.action, i, &wins);
                return true;
            }
        }

        _ => {}
    }

    false
}

fn execute_command(
    line: &str,

    vocab: &Vocab,

    apps: &mut Vec<App>,

    aliases: &mut HashMap<String, String>,

    voice_inst: Option<&voice::Voice>,
) -> bool {
    let line = line.trim();

    if line.is_empty() {
        return false;
    }

    execute_single_command(line, vocab, apps, aliases, voice_inst)
}

#[allow(dead_code)]
fn run_text_mode(
    vocab: &Vocab,

    mut apps: &mut Vec<App>,

    mut aliases: &mut HashMap<String, String>,
) {
    println!("\nType commands: open <app> [left/right/fullscreen] / close <app> / play <song> / volume 50% / lock / shutdown");

    println!("Commands: 'apps', 'windows', 'q' to quit.\n");

    loop {
        let line = ask("> ");

        let line = line.trim();

        if line.eq_ignore_ascii_case("q") {
            break;
        }

        execute_command(line, vocab, &mut apps, &mut aliases, None);
    }
}

// ---------- main loop ----------

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

fn start_voice_engine(settings_state: Arc<Mutex<settings::AuraSettings>>, app: AppHandle) {
    let vocab = load_vocab();
    let mut aliases = load_aliases();
    let mut apps = scan_apps();
    let require_wake_word = true;

    loop {
        // When Voice is turned OFF (or inactive during --silent PC boot), keep zero mic handles,
        // zero CornerPill windows/threads, and minimal idle memory.
        while !settings_state.lock().unwrap().module_should_run_now("voice") {
            voice::set_listening_allowed(false);
            std::thread::sleep(Duration::from_millis(300));
        }
        voice::set_listening_allowed(true);

        let pill = std::sync::Arc::new(pill::CornerPill::new());
        pill::spawn_pill_window(pill.shared.clone());

        let mut v = match voice::Voice::new() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to initialize voice: {e}");
                pill.destroy();
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        v.calibrate();

        loop {
            if !settings_state.lock().unwrap().module_should_run_now("voice") {
                break;
            }

            if MIC_MUTED.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            match v.wait_for_wake_word() {
                Ok(Some(heard)) => {
                    let raw = heard.text.trim();

                    if raw.is_empty() {
                        continue;
                    }

                    let (current_wake, pill_enabled) = {
                        let s = settings_state.lock().unwrap();

                        (s.wake_word.clone(), s.corner_pill)
                    };

                    let show_pill = pill_enabled;

                    let matched_cmd = std::iter::once(raw)
                        .chain(heard.alternatives.iter().map(|s| s.trim()))
                        .filter(|s| !s.is_empty())
                        .find_map(|cand| extract_command(cand, require_wake_word, &current_wake));

                    if let Some(cmd) = matched_cmd {
                        if cmd.is_empty() {
                            unsafe {
                                MessageBeep(0x00000040);
                            }

                            if show_pill {
                                pill.show_listening();
                            } else {
                                pill.hide();
                            }

                            let _ = app.emit(
                                "voice-event",
                                serde_json::json!({
                                    "type": "wake",
                                    "text": "Listening for your command...",
                                    "status": "Listening"
                                }),
                            );

                            v.reset_buffers();

                            if let Ok(Some(next_heard)) = v.wait_for_command() {
                                let next_cmd = next_heard.text.trim();

                                if !next_cmd.is_empty() {
                                    if show_pill {
                                        pill.show_speech(next_cmd);
                                    }

                                    let _ = app.emit(
                                        "voice-event",
                                        serde_json::json!({
                                            "type": "speech",
                                            "text": format!("Heard: \"{}\"", next_cmd),
                                            "status": "Processing"
                                        }),
                                    );

                                    std::thread::sleep(Duration::from_millis(500));

                                    let res = execute_command(
                                        next_cmd,
                                        &vocab,
                                        &mut apps,
                                        &mut aliases,
                                        Some(&v),
                                    );

                                    if show_pill {
                                        pill.show_action(next_cmd, res);
                                    }

                                    let _ = app.emit("voice-event", serde_json::json!({
                                        "type": "action",
                                        "text": format!("Action: \"{}\" ({})", next_cmd, if res { "Success" } else { "Failed" }),
                                        "status": if res { "Completed" } else { "Unknown" }
                                    }));
                                } else if show_pill {
                                    pill.hide();
                                }
                            } else if show_pill {
                                pill.hide();
                            }
                        } else {
                            unsafe {
                                MessageBeep(0x00000040);
                            }

                            if show_pill {
                                pill.show_speech(&cmd);
                            } else {
                                pill.hide();
                            }

                            let _ = app.emit(
                                "voice-event",
                                serde_json::json!({
                                    "type": "speech",
                                    "text": format!("Heard: \"{}\"", cmd),
                                    "status": "Processing"
                                }),
                            );

                            std::thread::sleep(Duration::from_millis(500));

                            let res = execute_command(&cmd, &vocab, &mut apps, &mut aliases, Some(&v));

                            if show_pill {
                                pill.show_action(&cmd, res);
                            }

                            let _ = app.emit("voice-event", serde_json::json!({
                                "type": "action",
                                "text": format!("Action: \"{}\" ({})", cmd, if res { "Success" } else { "Failed" }),
                                "status": if res { "Completed" } else { "Unknown" }
                            }));
                        }
                    }
                }

                Ok(None) => {}

                Err(e) => {
                    eprintln!("Voice error: {e}");
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }

        // Shutdown Voice resources completely when turned OFF: close mic stream + destroy pill HWND + trim RAM.
        voice::set_listening_allowed(false);
        pill.destroy();
        drop(v);
        unsafe {
            let _ = windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
            );
        }
    }
}

fn show_settings_window(app: &AppHandle) {
    let first_unlock = !settings::is_boot_unlocked();
    settings::unlock_boot();
    if first_unlock {
        let state = app.state::<Arc<Mutex<settings::AuraSettings>>>();
        let current = state.lock().unwrap().clone();
        voice::set_listening_allowed(current.module_should_run_now("voice"));
        modules::circle_search::sync_with_settings(&current, true);
    }
    if let Some(w) = app.get_webview_window("main") {
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
    settings::save_settings(&mut normalized);

    *state.lock().unwrap() = normalized.clone();
    voice::set_listening_allowed(normalized.module_should_run_now("voice"));
    modules::circle_search::sync_with_settings(&normalized, true);

    true
}

#[tauri::command]
fn close_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
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

#[tauri::command]
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

    let settings_state = Arc::new(Mutex::new(settings::load_settings()));

    // Background voice engine is spawned inside setup with AppHandle
    // Start HTTP server on 41780 (for external bridge & compatibility)
    server::start_server(settings_state.clone());

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
            get_settings,
            save_settings,
            close_settings,
            check_update,
            apply_update,
            get_mic_mute,
            set_mic_mute,
            toggle_mic_mute,
            open_browser,
            modules::circle_search::trigger_circle_search,
            modules::circle_search::overlay_ready,
            modules::circle_search::get_frozen_frame,
            modules::circle_search::close_circle_search_overlay,
            modules::circle_search::process_circle_selection,
            modules::circle_search::lens_visual_search,
            modules::circle_search::visual_search_selection,
            modules::circle_search::translate_selection,
            modules::circle_search::google_translate_image,
            modules::circle_search::recognize_desktop_music,
            modules::circle_search::copy_circle_text,
            modules::circle_search::search_circle_text,
            modules::circle_search::open_external_url,
            modules::circle_search::get_circle_search_status
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let sc = settings_state.clone();
            let initial_settings = sc.lock().unwrap().clone();

            modules::circle_search::init(handle.clone(), &initial_settings);

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
                    "quit" => app.exit(0),
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

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            let _ = tray_builder.build(app);

            // Main window starts hidden ("visible": false) so --silent boot never flashes a popup.
            let is_silent = settings::is_autostart_boot();

            if !is_silent {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();

                    let _ = window.hide();
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
