//! System power/hardware, scrolling, window management, and vocabulary app launcher handlers.

use std::collections::HashMap;
use std::process::Command;

use crate::app_launcher::{launch_app_smart, App};
use crate::file_search::handle_file_explorer;
use crate::input::scroll;
use crate::modules;
use crate::spotify::{launch_spotify, open_spotify_app};
use crate::vocab::Vocab;
use crate::voice;
use crate::volume::handle_volume_command;
use crate::wake_word::{norm, score};
use crate::window_mgr::{
    close_app, list_windows, minimize_app, resolve, window_action, SC_MINIMIZE, WM_SYSCOMMAND,
};
use crate::youtube::open_youtube;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, PostMessageW, SetForegroundWindow, ShowWindow, SW_MINIMIZE,
    SW_RESTORE, SW_SHOW,
};

use super::parser::{handle_timer_command, parse, parse_power_timer};

/// Execute system power, native OS commands, brightness, volume, screenshot, timer, and Circle to Search.
pub(crate) fn try_system_command(lnorm: &str) -> Option<bool> {
    if matches!(
        lnorm,
        "lock" | "lock pc" | "lock computer" | "lock screen"
    ) {
        println!("🔒 Locking PC Workstation...");
        unsafe {
            windows_sys::Win32::System::Shutdown::LockWorkStation();
        }
        return Some(true);
    }

    if let Some((reboot, secs)) = parse_power_timer(lnorm) {
        let action = if reboot { "Restarting" } else { "Shutting down" };
        println!("🛑 {action} PC in {secs}s (Say 'cancel shutdown' to abort)...");
        let ok = crate::native_cmds::shutdown_after(reboot, secs);
        return Some(ok);
    }

    if let Some(ok) = crate::native_cmds::try_handle(lnorm) {
        return Some(ok);
    }

    if matches!(
        lnorm,
        "shutdown" | "shutdown pc" | "turn off pc" | "shut down pc"
    ) {
        println!("🛑 Shutting down PC in 5 seconds (Say 'cancel shutdown' to abort)...");
        let _ = Command::new("shutdown").args(["/s", "/t", "5"]).spawn();
        return Some(true);
    }

    if matches!(
        lnorm,
        "restart" | "restart pc" | "reboot pc" | "reboot"
    ) {
        println!("🔄 Restarting PC in 5 seconds (Say 'cancel shutdown' to abort)...");
        let _ = Command::new("shutdown").args(["/r", "/t", "5"]).spawn();
        return Some(true);
    }

    if matches!(
        lnorm,
        "cancel shutdown" | "abort shutdown" | "stop shutdown"
    ) {
        println!("✅ Shutdown aborted!");
        let _ = Command::new("shutdown").args(["/a"]).spawn();
        return Some(true);
    }

    match modules::circle_search::handle_voice_command(lnorm) {
        Ok(true) => return Some(true),
        Ok(false) => {}
        Err(e) => {
            eprintln!("Circle to Search voice command failed: {e}");
            return Some(false);
        }
    }

    if let Some(ok) = crate::screenshot::handle_voice_command(lnorm) {
        return Some(ok);
    }

    // Brightness must precede volume so percentages aren't treated as audio volume.
    if let Some(ok) = crate::brightness::handle_voice_command(lnorm) {
        return Some(ok);
    }

    if let Some(ok) = handle_timer_command(lnorm) {
        return Some(ok);
    }

    if handle_volume_command(lnorm) {
        return Some(true);
    }

    None
}

/// Handle mouse wheel scrolling, window minimize, and explicit app close phrases.
pub(crate) fn try_scroll_minimize_close(lnorm: &str) -> Option<bool> {
    if lnorm.contains("scroll down")
        || lnorm == "scroll"
        || lnorm == "page down"
        || lnorm.contains("scroll niche")
        || lnorm.contains("halka scroll")
    {
        println!("📜 Scrolling down...");
        scroll(-240);
        return Some(true);
    }

    if lnorm.contains("scroll up") || lnorm == "page up" || lnorm.contains("scroll upor") {
        println!("📜 Scrolling up...");
        scroll(240);
        return Some(true);
    }

    let is_minimize = lnorm.starts_with("minimize ")
        || lnorm.ends_with(" minimize")
        || lnorm.contains(" minimize ")
        || lnorm.contains("choto koro")
        || lnorm.contains("hide ")
        || lnorm.ends_with(" hide")
        || lnorm == "minimize";

    if is_minimize {
        let mut target = lnorm.to_string();
        for w in [
            "minimize",
            "choto koro",
            "choto",
            "koro",
            "karo",
            "the",
            "app",
            "window",
            "hide",
            "please",
        ] {
            target = target.replace(w, " ");
        }
        let target = target.split_whitespace().collect::<Vec<_>>().join(" ");
        if target.is_empty() {
            unsafe {
                let fg = GetForegroundWindow();
                if !fg.is_null() {
                    PostMessageW(fg, WM_SYSCOMMAND, SC_MINIMIZE, 0);
                    ShowWindow(fg, SW_MINIMIZE);
                    println!("🪟 Minimized active window.");
                }
            }
            return Some(true);
        }
        return Some(minimize_app(&target));
    }

    let is_close = lnorm.starts_with("close ")
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

    if is_close && !is_media_stop {
        let mut target = lnorm.to_string();
        for w in [
            "close",
            "quit",
            "exit",
            "bondho koro",
            "band karo",
            "kete dao",
            "cut karo",
            "kato",
            "koro",
            "karo",
            "the",
            "app",
            "window",
            "please",
        ] {
            target = target.replace(w, " ");
        }
        let target = target.split_whitespace().collect::<Vec<_>>().join(" ");
        if !target.is_empty() {
            return Some(close_app(&target));
        }
    }

    None
}

/// Bring a matching open window to the foreground.
pub(crate) fn try_focus_window(lnorm: &str) -> Option<bool> {
    if !(lnorm.starts_with("focus ")
        || lnorm.starts_with("show ")
        || lnorm.contains("samne lao")
        || lnorm.contains("to front"))
    {
        return None;
    }

    let mut target_app = lnorm.to_string();
    for w in [
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
            ShowWindow(w.hwnd, if IsIconic(w.hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
            SetForegroundWindow(w.hwnd);
        }
        return Some(true);
    }

    None
}

fn launch_discord(apps: &[App]) {
    if let Some(a) = apps.iter().find(|a| norm(&a.name) == "discord") {
        launch_app_smart(a);
    } else {
        let _ = Command::new("explorer.exe").arg("discord://").spawn();
    }
}

/// Handle file explorer commands, debug app/window dumps, direct app names, and vocabulary actions.
pub(crate) fn try_app_and_vocab_command(
    raw_line: &str,
    lnorm: &str,
    vocab: &Vocab,
    apps: &mut [App],
    aliases: &mut HashMap<String, String>,
    voice_inst: Option<&voice::Voice>,
) -> bool {
    if handle_file_explorer(lnorm, voice_inst) {
        return true;
    }

    if raw_line.eq_ignore_ascii_case("apps") {
        for a in apps.iter().take(60) {
            println!("  {}", a.name);
        }
        println!("  ({} total)", apps.len());
        return true;
    }

    if raw_line.eq_ignore_ascii_case("windows") {
        for w in list_windows() {
            println!("  [{}] {}", w.exe, w.title);
        }
        return true;
    }

    let Some(p) = parse(lnorm, vocab) else {
        let single_norm = norm(lnorm);
        match single_norm.as_str() {
            "spotify" => {
                open_spotify_app();
                return true;
            }
            "youtube" => {
                open_youtube(apps);
                return true;
            }
            "chrome" => {
                if let Some(a) = apps.iter().find(|a| norm(&a.name).contains("chrome")) {
                    launch_app_smart(a);
                } else {
                    let _ = Command::new("explorer.exe").arg("chrome.exe").spawn();
                }
                return true;
            }
            "discord" => {
                launch_discord(apps);
                return true;
            }
            "all song" | "all songs" => {
                launch_spotify(None);
                return true;
            }
            _ => {}
        }

        // Exact app-name match only: never launch random apps for unrecognized sentences.
        if !single_norm.is_empty() {
            if let Some(a) = apps.iter().find(|a| norm(&a.name) == single_norm) {
                println!("🚀 Launching app by exact name: {}", a.name);
                launch_app_smart(a);
                return true;
            }
        }

        println!("⚠️ Action not recognized in: \"{}\"", raw_line);
        return false;
    };

    if p.target.is_empty() {
        println!("❓ What should I {}?", p.action);
        return false;
    }

    match p.action.as_str() {
        "open" => {
            let target_norm = norm(&p.target);
            if target_norm == "youtube" || target_norm.starts_with("youtube ") {
                open_youtube(apps);
                return true;
            }
            if target_norm == "spotify" {
                open_spotify_app();
                return true;
            }
            if target_norm == "discord" {
                launch_discord(apps);
                return true;
            }

            let q = aliases
                .get(&target_norm)
                .cloned()
                .unwrap_or_else(|| p.target.clone());
            let label = |i: usize| apps[i].name.clone();
            let sc = |q: &str, i: usize| score(q, &apps[i].name);

            if let Some(i) = resolve(&p.target, apps.len(), &label, &sc, 0.75, aliases, voice_inst)
            {
                if score(&q, &apps[i].name).max(score(&p.target, &apps[i].name)) < 0.70 {
                    println!(
                        "❌ \"{}\" app list-e nai (closest: {})",
                        p.target, apps[i].name
                    );
                    return false;
                }
                launch_app_smart(&apps[i]);
                return true;
            }
        }
        "close" => return close_app(&p.target),
        "minimize" => return minimize_app(&p.target),
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

            if let Some(i) = resolve(&p.target, wins.len(), &label, &sc, 0.68, aliases, voice_inst)
            {
                println!("🪟 Executing {} on: {}", p.action, label(i));
                window_action(&p.action, i, &wins);
                return true;
            }
        }
        _ => {}
    }

    false
}
