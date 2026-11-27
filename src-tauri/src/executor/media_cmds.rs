//! Media playback (WinRT GSMTC), YouTube/Spotify search & play, and video fullscreen controls.

use std::time::Duration;

use crate::app_launcher::{open_url_in_target_browser, parse_search_intent, App, LAST_YOUTUBE_VIDEOS};
use crate::input::send_key;
use crate::media_native;
use crate::spotify::{is_sp_target, launch_spotify, open_spotify_app, sp_previous, SP_SESSION};
use crate::wake_word::{norm, strip_phrases, url_encode};
use crate::window_mgr::{close_app_by_name, list_windows};
use crate::youtube::{
    is_yt_target, open_youtube, open_youtube_app, play_youtube, search_youtube, yt_in_window,
    yt_pause_resume, yt_skip, YT_SESSION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

use super::parser::media_nav_intent;

fn pick_youtube_result(apps: &[App], ordinal: usize, fallback_play: bool) -> bool {
    let pick_str = ordinal.to_string();
    if yt_in_window("pick", &pick_str) {
        return true;
    }
    if let Ok(lock) = LAST_YOUTUBE_VIDEOS.lock() {
        if let Some(video_id) = lock.get(ordinal - 1) {
            let url = format!("https://www.youtube.com/watch?v={video_id}");
            println!("▶️ Auto-playing #{} video in YouTube App: {url}", ordinal);
            open_youtube_app(apps, Some(&url));
            return true;
        }
    }
    if fallback_play {
        println!("▶️ Auto-playing YouTube playback via native media controls...");
        media_native::control_media(media_native::MediaAction::Play, Some(YT_SESSION));
    }
    true
}

/// Select the 1st, 2nd, or 3rd cached YouTube search result.
pub(crate) fn try_video_pick(lnorm: &str, apps: &[App]) -> Option<bool> {
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
        return Some(pick_youtube_result(apps, 1, true));
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
        return Some(pick_youtube_result(apps, 2, false));
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
        return Some(pick_youtube_result(apps, 3, false));
    }

    None
}

fn resolve_media_target(cmd: &str, youtube_open: bool, spotify_open: bool) -> Option<&'static str> {
    if cmd.contains("youtube") || cmd.contains("video") {
        return Some(YT_SESSION);
    }
    if cmd.contains("spotify")
        || cmd.contains("song")
        || cmd.contains("gana")
        || cmd.contains("gaan")
    {
        return Some(SP_SESSION);
    }

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

/// Handle video fullscreen enter/exit and WinRT GSMTC transport actions (prev/next/pause/play).
pub(crate) fn try_fullscreen_and_transport(
    lnorm: &str,
    youtube_open: bool,
    spotify_open: bool,
) -> Option<bool> {
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
                ShowWindow(w.hwnd, if IsIconic(w.hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
                SetForegroundWindow(w.hwnd);
            }
            std::thread::sleep(Duration::from_millis(80));
            send_key(0x46);
            println!("✅ YouTube video fullscreen toggled ('F')");
        } else {
            send_key(0x46);
            println!("✅ Fullscreen key ('F') sent to active window");
        }
        return Some(true);
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
        return Some(true);
    }

    if media_nav_intent(lnorm) == Some(false)
        || lnorm.contains("previous")
        || lnorm.contains("prev track")
        || lnorm.contains("pichla")
        || lnorm.contains("prevuse")
        || lnorm.contains("play previous")
        || lnorm.contains("prev video")
        || lnorm.contains("prev song")
    {
        let target = resolve_media_target(lnorm, youtube_open, spotify_open);
        println!("⏮️ Media: Previous on {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_skip(false);
        } else if is_sp_target(target) {
            sp_previous();
        } else {
            media_native::control_media(media_native::MediaAction::Previous, target);
        }
        return Some(true);
    }

    let is_next = media_nav_intent(lnorm) == Some(true)
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
        let target = resolve_media_target(lnorm, youtube_open, spotify_open);
        println!("⏭️ Media: Next on {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_skip(true);
        } else {
            media_native::control_media(media_native::MediaAction::Next, target);
        }
        return Some(true);
    }

    let is_pause = lnorm.contains("pause")
        || lnorm.contains("stop music")
        || lnorm == "stop"
        || lnorm.contains("band karo")
        || lnorm.contains("ruk jao")
        || lnorm.contains("puse");

    if is_pause {
        let target = resolve_media_target(lnorm, youtube_open, spotify_open);
        println!("⏸️ Media: Pausing {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_pause_resume(true);
        } else {
            media_native::control_media(media_native::MediaAction::Pause, target);
        }
        return Some(true);
    }

    if lnorm.contains("resume")
        || lnorm.contains("unpause")
        || lnorm == "play"
        || lnorm == "continue"
    {
        let target = resolve_media_target(lnorm, youtube_open, spotify_open);
        println!("▶️ Media: Resuming {:?}", target.unwrap_or("active session"));
        if is_yt_target(target) {
            yt_pause_resume(false);
        } else {
            media_native::control_media(media_native::MediaAction::Play, target);
        }
        return Some(true);
    }

    None
}

/// Handle web/YouTube/Spotify search, compound open+play, play song requests, and quick YouTube/Spotify launch/close.
pub(crate) fn try_search_and_play(
    lnorm: &str,
    apps: &[App],
    youtube_open: bool,
    spotify_open: bool,
) -> Option<bool> {
    if let Some((service, query)) = parse_search_intent(lnorm) {
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
            return Some(true);
        }

        match service.as_str() {
            "spotify" => launch_spotify(Some(&query)),
            "youtube" => search_youtube(apps, &query),
            _ => {
                let url = format!("https://www.google.com/search?q={}", url_encode(&query));
                println!("🌐 Searching in {}: {}", service, query);
                open_url_in_target_browser(&service, &url);
            }
        }
        return Some(true);
    }

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
            let clean = strip_phrases(
                lnorm,
                &[
                    "open", "kholo", "and", "aur", "ar", "please", "on youtube", "in youtube",
                    "youtube mein", "youtube me", "youtube par", "youtube", "on spotify",
                    "in spotify", "spotify mein", "spotify me", "spotify par", "spotify", "play",
                    "the song", "song", "gana", "a song", "chalao", "chala", "bajao", "karo",
                    "koro",
                ],
            );
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
            return Some(true);
        }
    }

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

        let clean = strip_phrases(
            lnorm,
            &[
                "play", "the song", "song", "gana", "a song", "ei gaan ta", "ei gan ta",
                "ei gaan", "ei gan", "gaan ta", "gan ta", "gaan", "on youtube", "in youtube",
                "youtube mein", "youtube me", "youtube par", "youtube", "on spotify", "in spotify",
                "spotify mein", "spotify me", "spotify par", "spotify", "karo", "koro", "kardo",
                "chalao", "chala", "bajao", "please", "dekhao", "ei", "ta", "eta", "ota",
            ],
        );
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
            launch_spotify(query_opt);
        }
        return Some(true);
    }

    if (lnorm.contains("open") || lnorm.contains("kholo")) && lnorm.contains("youtube") {
        open_youtube(apps);
        return Some(true);
    }

    if (lnorm.contains("open") || lnorm.contains("kholo")) && lnorm.contains("spotify") {
        open_spotify_app();
        return Some(true);
    }

    let close_verb = lnorm.contains("close")
        || lnorm.contains("band")
        || lnorm.contains("cut")
        || lnorm.contains("kete");

    if close_verb && lnorm.contains("youtube") {
        close_app_by_name("youtube");
        return Some(true);
    }

    if close_verb && lnorm.contains("spotify") {
        close_app_by_name("spotify");
        return Some(true);
    }

    None
}
