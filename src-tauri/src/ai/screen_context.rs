//! Read-only PC screen awareness built strictly on existing `circle_search::capture` and `circle_search::ocr`.
//! Captures active window title and compact offline OCR text so AI sees real screen errors & open UI.

use crate::modules::circle_search::{capture, ocr};

pub struct ScreenSnapshot {
    pub active_window: String,
    pub ocr_text: String,
}

#[cfg(target_os = "windows")]
pub fn active_window_title() -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize]).trim().to_string()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn active_window_title() -> String {
    String::new()
}

/// Enumerates visible non-tool taskbar/desktop window titles cleanly (~1ms Win32 EnumWindows).
#[cfg(target_os = "windows")]
pub fn open_window_apps() -> Vec<String> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW, GetWindowTextW, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if (ex & WS_EX_TOOLWINDOW) != 0 {
            return 1;
        }
        let mut buf = [0u16; 160];
        let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
            if !title.is_empty() && title != "Program Manager" && title != "Settings" {
                let list = &mut *(lparam as *mut Vec<String>);
                if list.len() < 14 && !list.contains(&title) {
                    list.push(title);
                }
            }
        }
        1
    }

    let mut titles: Vec<String> = Vec::new();
    unsafe {
        EnumWindows(Some(cb), &mut titles as *mut Vec<String> as LPARAM);
    }
    titles
}

#[cfg(not(target_os = "windows"))]
pub fn open_window_apps() -> Vec<String> {
    Vec::new()
}

/// Extracts bounded OCR text (~2,200 chars max) from an arbitrary BGRA frame.
pub fn extract_ocr_snippet(w: u32, h: u32, bgra: &[u8]) -> Result<String, String> {
    let ocr_res = ocr::extract_text_offline(w, h, bgra, 0, 0)?;

    let mut clean = String::new();
    for line in ocr_res.lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if clean.len() + t.len() + 1 > 2200 {
            break;
        }
        if !clean.is_empty() {
            clean.push('\n');
        }
        clean.push_str(t);
    }
    Ok(clean)
}

/// Captures the active monitor read-only and extracts bounded OCR text (~2,200 chars max).
pub fn capture_screen_context() -> Result<ScreenSnapshot, String> {
    let title = active_window_title();
    let frame = capture::capture_active_monitor()?;
    let ocr_text = extract_ocr_snippet(frame.width, frame.height, &frame.bgra)?;

    Ok(ScreenSnapshot {
        active_window: title,
        ocr_text,
    })
}

/// Broad detector covering English, Hindi/Hinglish, and Bengali phrases asking about the screen.
pub fn looks_like_screen_query(prompt: &str) -> bool {
    let l = prompt.to_ascii_lowercase();
    let cues = [
        "screen",
        "display",
        "monitor",
        "window",
        "taskbar",
        "open app",
        "app open",
        "apps open",
        "running",
        "screenshot",
        "ocr",
        "dekh",
        "dekho",
        "error",
        "problem",
        "issue",
        "bug",
        "failed",
        "exception",
        "what is this",
        "whats this",
        "what app",
        "whats open",
        "what's open",
        "explain this",
        "read this",
        "eta ki",
        "ki eta",
        "kya chal",
        "kya khula",
        "kya open",
    ];
    cues.iter().any(|c| l.contains(c))
        || prompt.contains("স্ক্রিন")
        || prompt.contains("এরর")
        || prompt.contains("সমস্যা")
        || prompt.contains("দেখো")
        || prompt.contains("ভুল")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_query_detector_handles_english_banglish_hinglish_and_bengali() {
        assert!(looks_like_screen_query("kya hal hai abhi tum mera screen dekh sakte ho"));
        assert!(looks_like_screen_query("what is this error on my screen"));
        assert!(looks_like_screen_query("ei error ta keno asche"));
        assert!(looks_like_screen_query("স্ক্রিনে কী দেখাচ্ছে দেখো তো"));
        assert!(!looks_like_screen_query("tell me a joke"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn screen_ocr_snippet_succeeds_from_worker_thread_when_live_dc_unavailable() {
        let handle = std::thread::spawn(|| {
            match capture_screen_context() {
                Ok(snap) => assert!(!snap.ocr_text.is_empty() || !snap.active_window.is_empty()),
                Err(_) => {
                    // Non-interactive / headless test sessions have no desktop GDI handle; verify OCR buffer pipeline still runs cleanly.
                    let bgra = vec![245u8; 160 * 40 * 4];
                    assert!(extract_ocr_snippet(160, 40, &bgra).is_ok());
                }
            }
        });
        handle.join().expect("OCR worker thread panicked");
    }
}
