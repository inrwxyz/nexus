//! Desktop audio Shazam + SMTC track recognition for Circle to Search.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

use super::shazam::MusicRecognitionResult;
use super::{browser, hide_and_destroy_overlay, shazam, url_encode_query};

fn music_pill_slot() -> &'static Mutex<Option<Arc<crate::pill::CornerPill>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<crate::pill::CornerPill>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn music_pill() -> Arc<crate::pill::CornerPill> {
    let mut g = music_pill_slot().lock().unwrap();
    if let Some(p) = g.as_ref() {
        return p.clone();
    }
    let p = Arc::new(crate::pill::CornerPill::new());
    crate::pill::spawn_pill_window(p.shared.clone());
    *g = Some(p.clone());
    p
}

pub(crate) fn destroy_music_pill() {
    if let Some(p) = music_pill_slot().lock().unwrap().take() {
        p.destroy();
    }
}

async fn run_desktop_music_detection() -> MusicRecognitionResult {
    let audio = match tauri::async_runtime::spawn_blocking(shazam::capture_system_audio_4s)
        .await
        .map_err(|e| format!("Audio capture worker error: {e}"))
        .and_then(|r| r)
    {
        Ok(a) => a,
        Err(e) => {
            return MusicRecognitionResult {
                found: false,
                title: String::new(),
                artist: String::new(),
                cover_url: None,
                search_url: String::new(),
                source: "loopback".to_string(),
                message: e,
            };
        }
    };

    let mut result = if audio.samples_16k.len() >= 8000 && audio.peak >= 0.0005 {
        let samples_full = audio.samples_16k;
        let sample_ms = ((samples_full.len() as f64 / 16000.0) * 1000.0).round() as u64;
        let tail_slice = if samples_full.len() > 16000 * 4 {
            Some(samples_full[samples_full.len() - 16000 * 4..].to_vec())
        } else {
            None
        };

        let mut res = match tauri::async_runtime::spawn_blocking(move || {
            shazam::build_shazam_signature(&samples_full)
        })
        .await
        {
            Ok(uri) => shazam::query_shazam(uri, sample_ms)
                .await
                .unwrap_or(MusicRecognitionResult {
                    found: false,
                    title: String::new(),
                    artist: String::new(),
                    cover_url: None,
                    search_url: String::new(),
                    source: "shazam".to_string(),
                    message: "Shazam lookup unreachable".to_string(),
                }),
            Err(_) => MusicRecognitionResult {
                found: false,
                title: String::new(),
                artist: String::new(),
                cover_url: None,
                search_url: String::new(),
                source: "shazam".to_string(),
                message: "Fingerprint worker error".to_string(),
            },
        };

        if !res.found {
            if let Some(tail) = tail_slice {
                let tail_ms = ((tail.len() as f64 / 16000.0) * 1000.0).round() as u64;
                if let Ok(uri2) = tauri::async_runtime::spawn_blocking(move || {
                    shazam::build_shazam_signature(&tail)
                })
                .await
                {
                    if let Ok(r2) = shazam::query_shazam(uri2, tail_ms).await {
                        if r2.found {
                            res = r2;
                        }
                    }
                }
            }
        }
        res
    } else {
        MusicRecognitionResult {
            found: false,
            title: String::new(),
            artist: String::new(),
            cover_url: None,
            search_url: String::new(),
            source: "loopback".to_string(),
            message: "Desktop audio is silent".to_string(),
        }
    };

    if !result.found {
        if let Some((title, artist)) =
            tauri::async_runtime::spawn_blocking(shazam::query_active_smtc_track)
                .await
                .unwrap_or(None)
        {
            let q = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} {title}")
            };
            let display = if artist.is_empty() {
                title.clone()
            } else {
                format!("{artist} - {title}")
            };
            result = MusicRecognitionResult {
                found: true,
                title,
                artist,
                cover_url: None,
                search_url: format!("https://www.google.com/search?q={}", url_encode_query(&q)),
                source: "system_media".to_string(),
                message: format!("Now playing: {display}"),
            };
        }
    }

    result
}

pub(crate) async fn recognize_desktop_music_impl(app: AppHandle) -> Result<MusicRecognitionResult, String> {
    hide_and_destroy_overlay(&app);

    let pill = music_pill();
    pill.show_speech("Listening for music…");

    tauri::async_runtime::spawn(async move {
        let _ = tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(240));
        })
        .await;
        let result = run_desktop_music_detection().await;

        if result.found && !result.search_url.is_empty() {
            let label = if result.artist.is_empty() {
                result.title.clone()
            } else {
                format!("{} - {}", result.title, result.artist)
            };
            pill.show_action(&label, true);
            let _ = browser::open_url(&result.search_url);
        } else {
            let miss_msg = if result.message.is_empty() {
                "No song detected".to_string()
            } else {
                result.message.clone()
            };
            pill.show_speech(&miss_msg);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1800));
                pill.hide();
            });
        }
    });

    Ok(MusicRecognitionResult {
        found: false,
        title: String::new(),
        artist: String::new(),
        cover_url: None,
        search_url: String::new(),
        source: "background".to_string(),
        message: "Unfroze screen and listening in background".to_string(),
    })
}
