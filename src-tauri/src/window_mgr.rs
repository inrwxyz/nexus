use std::collections::HashMap;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW,
    SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, WM_CLOSE, WS_EX_TOOLWINDOW,
};
use crate::media_native;
use crate::vocab::save_aliases;
use crate::voice;
use crate::wake_word::{ask, norm, pick, score};
use crate::youtube::YT_SESSION;

pub const WM_SYSCOMMAND: u32 = 0x0112;
pub const SC_MINIMIZE: usize = 0xF020;

#[derive(Clone, Debug)]
pub struct Win {
    pub hwnd: HWND,
    pub title: String,
    pub exe: String,
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

pub fn list_windows() -> Vec<Win> {
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

pub fn is_app_open(name: &str) -> bool {
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

pub fn window_action(action: &str, i: usize, wins: &[Win]) {
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

pub fn resolve<F>(
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

pub fn kill_processes_by_name(exe_name: &str) -> bool {
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

pub fn close_app(name: &str) -> bool {
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

pub fn minimize_app(target: &str) -> bool {
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

pub fn close_app_by_name(name: &str) {
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
