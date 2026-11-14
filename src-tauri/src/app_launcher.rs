use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use crate::media_native;
use crate::safety;
use crate::wake_word::{norm, strip_phrases};
use crate::window_mgr::{is_app_open, list_windows};
use crate::youtube::YT_SESSION;


#[derive(Clone, Debug)]
pub struct App {
    pub name: String,
    pub id: String,
}

pub fn scan_apps() -> Vec<App> {
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

pub fn launch(a: &App) {
    let _ = Command::new("explorer.exe")
        .arg(&a.id)
        .spawn();
}

pub static LAST_YOUTUBE_VIDEOS: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

pub fn foreground_browser_exe() -> Option<(String, String)> {
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

pub fn find_running_browser_exe(target_stem: Option<&str>) -> Option<(String, String)> {
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

pub fn open_url_in_target_browser(service: &str, url: &str) {
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

pub fn contains_word_phrase(haystack: &str, phrase: &str) -> bool {
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
pub fn parse_search_intent(text: &str) -> Option<(String, String)> {
    let yt_open = is_app_open("youtube");
    let sp_open = is_app_open("spotify");
    let yt_session_active = media_native::has_session(YT_SESSION);
    parse_search_intent_with_state(text, yt_open, sp_open, yt_session_active)
}

// ---------- window management (win32) ----------


pub fn launch_app_smart(a: &App) {
    println!("🚀 Launching: {}", a.name);
    launch(a);
}
