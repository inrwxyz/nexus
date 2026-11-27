use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING,
};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

pub const PIPE_NAME: &str = r"\\.\pipe\InexIslandNexus";
const MAX_QUEUE: usize = 64;
const CONNECT_TIMEOUT_MS: u32 = 300;

static SEQ: AtomicI64 = AtomicI64::new(0);
static LAST_TS: AtomicU64 = AtomicU64::new(0);
// (json line, is_level_update)
static SENDER: OnceLock<SyncSender<(String, bool)>> = OnceLock::new();

/// Strictly monotonic millisecond timestamp so rapid events are never dropped or reordered by Island.
fn next_ts() -> u64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1);
    let mut prev = LAST_TS.load(Ordering::Relaxed);
    loop {
        let next = now_ms.max(prev + 1);
        match LAST_TS.compare_exchange_weak(prev, next, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => prev = actual,
        }
    }
}

/// Check if Inex Island executable exists on disk.
/// Mirrors Locker Pro's check across LocalAppData and ProgramFiles.
pub fn is_inex_island_installed() -> bool {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if !local.is_empty() {
        let p1 = PathBuf::from(&local).join("Inex Island").join("inex-island.exe");
        let p2 = PathBuf::from(&local).join("InexIsland").join("inex-island.exe");
        if p1.exists() || p2.exists() {
            return true;
        }
    }
    let prog = std::env::var("ProgramFiles").unwrap_or_default();
    if !prog.is_empty() {
        let p3 = PathBuf::from(&prog).join("Inex Island").join("inex-island.exe");
        let p4 = PathBuf::from(&prog).join("InexIsland").join("inex-island.exe");
        if p3.exists() || p4.exists() {
            return true;
        }
    }
    false
}

/// Check if the `inex-island` process is actively running.
pub fn is_inex_island_running() -> bool {
    crate::is_process_running("inex-island")
}

fn to_u16_vec(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn get_sender() -> &'static SyncSender<(String, bool)> {
    SENDER.get_or_init(|| {
        let (tx, rx) = sync_channel::<(String, bool)>(MAX_QUEUE);
        std::thread::Builder::new()
            .name("InexIslandBridge".to_string())
            .spawn(move || worker_loop(rx))
            .expect("Failed to spawn InexIslandBridge worker");
        tx
    })
}

fn worker_loop(rx: Receiver<(String, bool)>) {
    let pipe_name_w = to_u16_vec(PIPE_NAME);
    // Level updates arrive ~20x/s: a process scan per update would be wasteful, so their
    // "is Island running" answer is reused for a moment. Every other event checks fresh.
    let mut level_running = false;
    let mut level_checked: Option<std::time::Instant> = None;
    while let Ok((json, is_level)) = rx.recv() {
        // Discard immediately if Inex Island is not running to avoid hanging
        let running = if is_level {
            if level_checked.is_none_or(|t| t.elapsed() > Duration::from_millis(1500)) {
                level_running = is_inex_island_running();
                level_checked = Some(std::time::Instant::now());
            }
            level_running
        } else {
            is_inex_island_running()
        };
        if !running {
            continue;
        }

        let mut data = json.into_bytes();
        data.push(b'\n');

        // Deliver with short timeout and retry
        let mut delivered = false;
        for _ in 0..2 {
            unsafe {
                if WaitNamedPipeW(pipe_name_w.as_ptr(), CONNECT_TIMEOUT_MS) != 0 {
                    let handle = CreateFileW(
                        pipe_name_w.as_ptr(),
                        GENERIC_WRITE,
                        0,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        std::ptr::null_mut(),
                    );

                    if handle != INVALID_HANDLE_VALUE {
                        let mut written = 0u32;
                        let _ = WriteFile(
                            handle,
                            data.as_ptr(),
                            data.len() as u32,
                            &mut written,
                            std::ptr::null_mut(),
                        );
                        CloseHandle(handle);
                        delivered = true;
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        // Small pacing gap after non-level events so Island's spawned pipe readers don't overtake each other.
        if delivered && !is_level {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

/// Send a status update to Inex Island over the named pipe.
/// Status states emitted: "wake", "level", "thinking", "done", "fail", "cancel".
pub fn send_island_event(status: &str, text: Option<&str>) {
    let now_ms = next_ts();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);

    let payload = serde_json::json!({
        "event": "nexus_voice",
        "status": status,
        "text": text.unwrap_or(""),
        "seq": seq,
        "ts": now_ms
    });

    let json = match serde_json::to_string(&payload) {
        Ok(j) => j,
        Err(_) => return,
    };

    let tx = get_sender();
    // Non-blocking try_send: if the queue is full, drop cleanly without delaying the voice pipeline
    let _ = tx.try_send((json, false));
}

/// Live mic loudness 0..1 while the user is speaking (drives the Island listening bars).
pub fn notify_level(level: f32) {
    let now_ms = next_ts();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let payload = serde_json::json!({
        "event": "nexus_voice",
        "status": "level",
        "level": (level.clamp(0.0, 1.0) * 1000.0).round() / 1000.0,
        "seq": seq,
        "ts": now_ms
    });
    if let Ok(json) = serde_json::to_string(&payload) {
        // Dropped silently when the queue is full: a missed bar update is harmless
        let _ = get_sender().try_send((json, true));
    }
}

pub fn notify_wake() {
    send_island_event("wake", Some("Listening..."));
}

/// Switches Inex Island to its animated voice wave bars while AI TTS is speaking.
pub fn notify_speaking() {
    send_island_event("wake", Some("Speaking"));
}

pub fn notify_thinking(command: Option<&str>) {
    send_island_event("thinking", command);
}

/// Success -> "done" (check animation). Failure -> "fail" (cross animation, already handled by Island).
pub fn notify_done(success: bool) {
    if success {
        send_island_event("done", Some("Success"));
    } else {
        send_island_event("fail", Some("Failed"));
    }
}

/// Triggers Inex Island's timer with the given duration in seconds.
/// Deliberately omits `status` so Island opens its Timer HUD without triggering the voice bio-HUD overlay.
pub fn notify_timer(seconds: u32) {
    let now_ms = next_ts();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let mm = seconds / 60;
    let ss = seconds % 60;
    let label = if mm > 0 && ss > 0 {
        format!("Timer {}m {}s", mm, ss)
    } else if mm > 0 {
        format!("Timer {}m", mm)
    } else {
        format!("Timer {}s", ss)
    };

    let payload = serde_json::json!({
        "event": "nexus_voice",
        "action": "set_timer",
        "seconds": seconds,
        "text": label,
        "seq": seq,
        "ts": now_ms
    });

    if let Ok(json) = serde_json::to_string(&payload) {
        let tx = get_sender();
        let _ = tx.try_send((json, false));
    }
}

pub fn notify_cancel() {
    send_island_event("cancel", None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_generation() {
        let seq = SEQ.load(Ordering::SeqCst);
        let payload = serde_json::json!({
            "event": "nexus_voice",
            "status": "wake",
            "text": "Listening...",
            "seq": seq,
            "ts": 123456789u64
        });
        assert_eq!(payload["event"], "nexus_voice");
        assert_eq!(payload["status"], "wake");
    }
}
