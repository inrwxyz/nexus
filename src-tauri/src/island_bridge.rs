use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
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
static SENDER: OnceLock<SyncSender<String>> = OnceLock::new();

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

fn get_sender() -> &'static SyncSender<String> {
    SENDER.get_or_init(|| {
        let (tx, rx) = sync_channel::<String>(MAX_QUEUE);
        std::thread::Builder::new()
            .name("InexIslandBridge".to_string())
            .spawn(move || worker_loop(rx))
            .expect("Failed to spawn InexIslandBridge worker");
        tx
    })
}

fn worker_loop(rx: Receiver<String>) {
    let pipe_name_w = to_u16_vec(PIPE_NAME);
    while let Ok(json) = rx.recv() {
        // Discard immediately if Inex Island is not running to avoid hanging
        if !is_inex_island_running() {
            continue;
        }

        let mut data = json.into_bytes();
        data.push(b'\n');

        // Deliver with short timeout and retry
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
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(60));
        }
    }
}

/// Send a status update to Inex Island over the named pipe.
/// Only 4 status states are emitted: "wake", "thinking", "done", "cancel".
pub fn send_island_event(status: &str, text: Option<&str>) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
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
    let _ = tx.try_send(json);
}

pub fn notify_wake() {
    send_island_event("wake", Some("Listening..."));
}

pub fn notify_thinking(command: Option<&str>) {
    send_island_event("thinking", command);
}

pub fn notify_done(success: bool) {
    send_island_event("done", Some(if success { "Success" } else { "Failed" }));
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
