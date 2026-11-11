use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

fn get_manager() -> Option<GlobalSystemMediaTransportControlsSessionManager> {
    GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()
        .and_then(|op| op.get().ok())
}

#[allow(dead_code)]
pub fn has_session(target_app: &str) -> bool {
    let manager = match get_manager() {
        Some(m) => m,
        None => return false,
    };
    let sessions = match manager.GetSessions() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let count = sessions.Size().unwrap_or(0);
    let app = target_app.to_lowercase();
    for i in 0..count {
        if let Ok(sess) = sessions.GetAt(i) {
            if let Ok(id) = sess.SourceAppUserModelId() {
                let id_str = id.to_string_lossy().to_lowercase();
                if matches_target(&id_str, &app) {
                    return true;
                }
            }
        }
    }
    false
}

#[allow(dead_code)]
pub fn is_playing(target_app: Option<&str>) -> bool {
    let manager = match get_manager() {
        Some(m) => m,
        None => return false,
    };
    let sessions = match manager.GetSessions() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let count = sessions.Size().unwrap_or(0);
    let app_opt = target_app.map(|s| s.to_lowercase());

    for i in 0..count {
        if let Ok(sess) = sessions.GetAt(i) {
            let matches = match app_opt {
                Some(ref app) => {
                    let id_str = sess.SourceAppUserModelId().map(|h| h.to_string_lossy().to_lowercase()).unwrap_or_default();
                    matches_target(&id_str, app)
                }
                None => true,
            };
            if matches {
                if let Ok(info) = sess.GetPlaybackInfo() {
                    if let Ok(status) = info.PlaybackStatus() {
                        if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// Generic: `target` is one or more lowercase words separated by '|'
// (e.g. "spotify" or "chrome|msedge|brave"). Matches if the session's app id contains any of them.
// App-specific lists live in spotify.rs / youtube.rs, not here.
fn matches_target(app_id: &str, target: &str) -> bool {
    target.split('|').any(|t| !t.is_empty() && app_id.contains(t))
}

pub fn control_media(action: MediaAction, target_app: Option<&str>) -> bool {
    let manager = match get_manager() {
        Some(m) => m,
        None => {
            eprintln!("GSMTC: Failed to obtain GlobalSystemMediaTransportControlsSessionManager");
            return false;
        }
    };

    let sessions = match manager.GetSessions() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("GSMTC: GetSessions error: {e}");
            return false;
        }
    };

    let count = sessions.Size().unwrap_or(0);
    if count == 0 {
        println!("GSMTC: No active media sessions found");
        return false;
    }

    let mut target_session: Option<GlobalSystemMediaTransportControlsSession> = None;
    let hint = target_app.map(|s| s.to_lowercase());

    // If only one media session exists globally, it is unambiguously our target
    if count == 1 {
        target_session = sessions.GetAt(0).ok();
    } else {
        // 1. If explicit app hint given, match session by AppUserModelId
        if let Some(ref app) = hint {
            for i in 0..count {
                if let Ok(sess) = sessions.GetAt(i) {
                    if let Ok(id) = sess.SourceAppUserModelId() {
                        let id_str = id.to_string_lossy().to_lowercase();
                        if matches_target(&id_str, app) {
                            target_session = Some(sess);
                            break;
                        }
                    }
                }
            }
        }

        // 2. If no target specified or not found:
        // For Pause/Next/Prev, prefer session that is currently PLAYING
        if target_session.is_none() && (action == MediaAction::Pause || action == MediaAction::Next || action == MediaAction::Previous || action == MediaAction::Toggle) {
            for i in 0..count {
                if let Ok(sess) = sessions.GetAt(i) {
                    if let Ok(info) = sess.GetPlaybackInfo() {
                        if let Ok(status) = info.PlaybackStatus() {
                            if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
                                target_session = Some(sess);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 3. For Play/Resume, prefer session that is currently PAUSED
        if target_session.is_none() && action == MediaAction::Play {
            for i in 0..count {
                if let Ok(sess) = sessions.GetAt(i) {
                    if let Ok(info) = sess.GetPlaybackInfo() {
                        if let Ok(status) = info.PlaybackStatus() {
                            if status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused {
                                target_session = Some(sess);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // 4. Fallback to CurrentSession or first session
        if target_session.is_none() {
            if let Ok(curr) = manager.GetCurrentSession() {
                target_session = Some(curr);
            } else if let Ok(first) = sessions.GetAt(0) {
                target_session = Some(first);
            }
        }
    }

    let session = match target_session {
        Some(s) => s,
        None => return false,
    };

    let app_id = session.SourceAppUserModelId().map(|h| h.to_string_lossy()).unwrap_or_default();
    println!("🎵 GSMTC: Executing {action:?} on session '{app_id}'");

    let result = match action {
        MediaAction::Play => session.TryPlayAsync().ok().and_then(|op| op.get().ok()).unwrap_or(false),
        MediaAction::Pause => session.TryPauseAsync().ok().and_then(|op| op.get().ok()).unwrap_or(false),
        MediaAction::Toggle => session.TryTogglePlayPauseAsync().ok().and_then(|op| op.get().ok()).unwrap_or(false),
        MediaAction::Next => session.TrySkipNextAsync().ok().and_then(|op| op.get().ok()).unwrap_or(false),
        MediaAction::Previous => session.TrySkipPreviousAsync().ok().and_then(|op| op.get().ok()).unwrap_or(false),
    };

    println!("🎵 GSMTC: {action:?} on '{app_id}' returned {result}");
    result
}
