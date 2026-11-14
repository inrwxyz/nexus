use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use crate::app_launcher::{App, LAST_YOUTUBE_VIDEOS, launch_app_smart, open_url_in_target_browser, parse_search_intent};
use crate::file_search::handle_file_explorer;
use crate::input::{scroll, send_key};
use crate::media_native;
use crate::modules;
use crate::spotify::{ is_sp_target, launch_spotify, open_spotify_app, sp_previous, SP_SESSION};
use crate::vocab::Vocab;
use crate::voice;
use crate::volume::{handle_volume_command};
use crate::wake_word::{norm, score, strip_phrases, url_encode};
use crate::window_mgr::{
    close_app, close_app_by_name, is_app_open,
    list_windows, minimize_app, resolve, window_action, SC_MINIMIZE, WM_SYSCOMMAND,
};
use crate::youtube::{ is_yt_target, open_youtube, open_youtube_app, play_youtube, search_youtube, yt_in_window, yt_pause_resume, yt_skip, YT_SESSION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, PostMessageW, SetForegroundWindow, ShowWindow,
    SW_MINIMIZE, SW_RESTORE, SW_SHOW,
};


#[derive(Clone, Debug)]
pub struct Parsed {
    pub action: String,
    pub target: String,
}

pub fn parse(text: &str, v: &Vocab) -> Option<Parsed> {
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


pub fn media_nav_intent(lnorm: &str) -> Option<bool> {
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

pub fn execute_single_command(
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

pub fn execute_command(
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
