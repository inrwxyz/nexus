//! Screen Mirror module: phone screen mirroring over USB, QR pairing, or IP + port.
//! Engine = Pure-Rust `nexus-mirror-engine` + Win32 Direct3D 11 / Media Foundation renderer.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nexus_mirror_engine::{
    AdbClient, DeviceInfo, MirrorEngine, ServerParams, WirelessConfig, WirelessManager,
};
use serde::Deserialize;

static MIRROR: Mutex<Option<Arc<super::mirror::MirrorHandle>>> = Mutex::new(None);
static MIRROR_META: Mutex<Option<ActiveMirrorMeta>> = Mutex::new(None);
static ADB: Mutex<Option<AdbClient>> = Mutex::new(None);
static SCRCPY_ENABLED: AtomicBool = AtomicBool::new(false);
static LAST_AUTO_SAVED: Mutex<Option<(String, u16)>> = Mutex::new(None);
static FORGOTTEN: AtomicBool = AtomicBool::new(false);
static QR_GEN: AtomicU64 = AtomicU64::new(0);
static QR_STATE: Mutex<(u64, String, String)> = Mutex::new((0, String::new(), String::new()));

#[derive(Clone, PartialEq, Eq)]
struct ActiveMirrorMeta {
    serial: Option<String>,
    max_size: u16,
    max_fps: u8,
    bitrate_mbps: u32,
    audio_enabled: bool,
    audio_buffer_ms: u16,
    audio_codec: String,
    audio_dup: bool,
    turn_screen_off: bool,
}

pub fn is_enabled() -> bool {
    SCRCPY_ENABLED.load(Ordering::SeqCst)
}

fn ensure_enabled() -> Result<(), String> {
    if is_enabled() {
        Ok(())
    } else {
        Err("Screen Mirror is turned off".into())
    }
}

/// Sync backend Scrcpy runtime state with `AuraSettings`.
/// When disabled, kills any active mirror, cancels QR pairing, and resets cached auto-save state
/// so no ADB discovery or background work runs until re-enabled.
pub fn sync_with_settings(settings: &crate::settings::AuraSettings) {
    let want = settings.module_should_run_now("scrcpy");
    SCRCPY_ENABLED.store(want, Ordering::SeqCst);
    if !want {
        sc_stop();
        sc_qr_cancel();
        if let Ok(mut last) = LAST_AUTO_SAVED.lock() {
            *last = None;
        }
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

fn prefs_path() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("Nexus");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("scrcpy-prefs.json");
    }
    PathBuf::from("scrcpy-prefs.json")
}

fn save_wireless_config(cfg: &WirelessConfig) {
    FORGOTTEN.store(false, Ordering::SeqCst);
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write("wireless_device.json", &json);
        let _ = std::fs::write(exe_dir().join("wireless_device.json"), &json);
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(local).join("Nexus");
            let _ = std::fs::create_dir_all(&p);
            let _ = std::fs::write(p.join("wireless_device.json"), &json);
        }
    }
}

/// Cached AdbClient: finding adb (spawning `adb version`, scanning folders) happens once,
/// not on every refresh / connect. A failed lookup is not cached, so it is retried next time.
fn adb_client() -> Result<AdbClient, String> {
    let mut g = ADB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(a) = g.as_ref() { return Ok(a.clone()); }
    let a = AdbClient::new().map_err(|e| e.to_string())?;
    *g = Some(a.clone());
    Ok(a)
}

/// Persist a wireless phone as the permanent device, but only for the fixed :5555 port and
/// only when it changed. Ephemeral Wireless-debugging ports (e.g. :35659) are session-only.
fn auto_save_permanent(devs: &[DeviceInfo]) {
    for d in devs {
        if !(d.is_wireless && d.state == "device") { continue; }
        let Some((ip, port)) = d.serial.split_once(':') else { continue };
        let Ok(p) = port.parse::<u16>() else { continue };
        if p != 5555 { continue; }
        let key = (ip.to_string(), p);
        {
            let mut last = LAST_AUTO_SAVED.lock().unwrap_or_else(|e| e.into_inner());
            if last.as_ref() == Some(&key) { continue; }
            *last = Some(key);
        }
        save_wireless_config(&WirelessConfig {
            ip: ip.to_string(),
            port: p,
            model: d.model.clone(),
            auto_reconnect: true,
            last_connected_at: format!("{:?}", std::time::SystemTime::now()),
        });
    }
}

#[tauri::command]
pub async fn sc_devices() -> Result<Vec<DeviceInfo>, String> {
    if !is_enabled() {
        return Ok(Vec::new());
    }
    let adb = adb_client()?;
    let mut devs = adb.list_devices().await.unwrap_or_default();

    // Auto-reconnect: If no device is currently attached, check if there is a saved
    // wireless device and try connecting quickly.
    if devs.is_empty() {
        if let Ok(Some(saved)) = sc_saved().await {
            if saved.auto_reconnect && !saved.ip.is_empty() {
                if let Ok(res) = WirelessManager::new(adb.clone()).connect(&saved.ip, saved.port).await {
                    if res.contains("connected to") || res.contains("already connected") {
                        if let Ok(d2) = adb.list_devices().await {
                            devs = d2;
                        }
                    }
                }
            }
        }
    }

    auto_save_permanent(&devs);
    Ok(devs)
}

#[tauri::command]
pub fn sc_kill_adb() {
    // Intentionally a no-op: System ADB belongs to the host environment / user CLI.
    // Nexus never terminates adb.exe to avoid killing user terminal sessions or causing IPC lags.
}

#[tauri::command]
pub async fn sc_connect(ip: String, port: u16) -> Result<String, String> {
    ensure_enabled()?;
    let r = WirelessManager::new(adb_client()?).connect(ip.trim(), port).await.map_err(|e| e.to_string())?;
    if r.contains("connected to") || r.contains("already connected") {
        let cfg = WirelessConfig {
            ip: ip.trim().to_string(),
            port,
            model: "Android Device".to_string(),
            auto_reconnect: true,
            last_connected_at: format!("{:?}", std::time::SystemTime::now()),
        };
        save_wireless_config(&cfg);
        Ok(format!("{}:{}", ip.trim(), port))
    } else {
        Err(r)
    }
}

fn forget_saved_wireless() {
    FORGOTTEN.store(true, Ordering::SeqCst);
    *LAST_AUTO_SAVED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut paths = vec![exe_dir().join("wireless_device.json"), PathBuf::from("wireless_device.json")];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        paths.push(PathBuf::from(local).join("Nexus").join("wireless_device.json"));
    }
    for p in paths { let _ = std::fs::remove_file(p); }
}

/// Removes a phone from Nexus: wireless -> `adb disconnect` + forget the saved IP so it
/// is not auto-reconnected. USB phones cannot be disconnected from software, so the UI just
/// hides them until they are re-plugged. Stops the mirror if it is running.
#[tauri::command]
pub async fn sc_remove_device(serial: String) -> Result<(), String> {
    sc_stop();
    let is_wireless = serial.contains(':') || serial.contains("_adb-tls-connect");
    if is_wireless {
        let adb = adb_client()?;
        let _ = adb_out(&adb, &["disconnect", &serial]).await;
        forget_saved_wireless();
    }
    Ok(())
}

#[tauri::command]
pub async fn sc_make_permanent(serial: String) -> Result<WirelessConfig, String> {
    ensure_enabled()?;
    let c = WirelessManager::new(adb_client()?).setup_permanent_wireless(&serial).await.map_err(|e| e.to_string())?;
    save_wireless_config(&c);
    Ok(c)
}

#[tauri::command]
pub async fn sc_saved() -> Result<Option<WirelessConfig>, String> {
    if !is_enabled() {
        return Ok(None);
    }
    let mut candidate_paths = vec![
        exe_dir().join("wireless_device.json"),
        PathBuf::from("wireless_device.json"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidate_paths.push(PathBuf::from(local).join("Nexus").join("wireless_device.json"));
    }
    for p in candidate_paths {
        if p.exists() {
            if let Ok(t) = std::fs::read_to_string(&p) {
                if let Ok(c) = serde_json::from_str::<WirelessConfig>(&t) {
                    if !c.ip.is_empty() {
                        return Ok(Some(c));
                    }
                }
            }
        }
    }
    // Fallback: check if Phone Link (Companion) recorded the phone's IP
    if FORGOTTEN.load(Ordering::SeqCst) { return Ok(None); }
    if let Some(ip) = crate::companion::get_last_phone_ip() {
        return Ok(Some(WirelessConfig {
            ip,
            port: 5555,
            model: "Phone".into(),
            auto_reconnect: true,
            last_connected_at: "".into(),
        }));
    }
    Ok(None)
}

#[tauri::command]
pub fn sc_load_prefs() -> serde_json::Value {
    std::fs::read_to_string(prefs_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn sc_save_prefs(prefs: serde_json::Value) {
    if let Ok(t) = serde_json::to_string_pretty(&prefs) { let _ = std::fs::write(prefs_path(), t); }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOpts {
    serial: Option<String>,
    max_size: u16,
    max_fps: u16,
    bitrate_mbps: u32,
    #[serde(default)]
    stay_awake: bool,
    turn_screen_off: bool,
    audio_enabled: Option<bool>,
    audio_buffer_ms: Option<u16>,
    audio_codec: Option<String>,
    audio_dup: Option<bool>,
}

fn sanitize_opts(opts: &StartOpts) -> (u16, u8, u32, u16, String, bool, bool) {
    let max_size = match opts.max_size {
        1280 => 1280,
        1920 => 1920,
        _ => 0,
    };
    let max_fps: u8 = if opts.max_fps >= 100 { 120 } else { 60 };
    let bitrate_mbps = match opts.bitrate_mbps {
        4 => 4,
        16 => 16,
        _ => 8,
    };
    let audio_buf = match opts.audio_buffer_ms.unwrap_or(120) {
        40 => 40,
        250 => 250,
        _ => 120,
    };
    // "auto" selects Opus -> AAC -> raw PCM via Windows Media Foundation.
    let audio_codec = match opts.audio_codec.as_deref() {
        Some("opus") => "opus",
        Some("aac") => "aac",
        Some("raw") | Some("flac") => "raw",
        _ => {
            if super::mirror_decoder::AudioMfDecoder::is_available(
                super::mirror_decoder::AudioCodecKind::Opus,
            ) {
                "opus"
            } else if super::mirror_decoder::AudioMfDecoder::is_available(
                super::mirror_decoder::AudioCodecKind::Aac,
            ) {
                "aac"
            } else {
                "raw"
            }
        }
    }
    .to_string();
    let audio_enabled = opts.audio_enabled.unwrap_or(true);
    let audio_dup = opts.audio_dup.unwrap_or(false);
    (max_size, max_fps, bitrate_mbps, audio_buf, audio_codec, audio_enabled, audio_dup)
}

#[tauri::command]
pub async fn sc_start(app: tauri::AppHandle, opts: StartOpts) -> Result<String, String> {
    ensure_enabled()?;
    let mut target_serial = opts.serial.clone();
    if target_serial.as_deref().unwrap_or("").is_empty() {
        if let Ok(devs) = sc_devices().await {
            let mut connected: Vec<_> = devs.into_iter().filter(|d| d.state == "device").collect();
            // Prioritize USB first, then standard :5555 Wi-Fi, then TLS
            connected.sort_by(|a, b| {
                let a_usb = !a.is_wireless;
                let b_usb = !b.is_wireless;
                if a_usb != b_usb { return b_usb.cmp(&a_usb); }
                let a_5555 = a.serial.ends_with(":5555");
                let b_5555 = b.serial.ends_with(":5555");
                b_5555.cmp(&a_5555)
            });
            if let Some(d) = connected.first() {
                target_serial = Some(d.serial.clone());
            }
        }
    }

    sc_stop(); // never keep two mirror windows

    let (max_size, max_fps, bitrate_mbps, audio_buf, audio_codec, audio_enabled, audio_dup) =
        sanitize_opts(&opts);

    let params = ServerParams {
        max_size,
        bit_rate: bitrate_mbps * 1_000_000,
        max_fps,
        audio: audio_enabled,
        audio_codec: audio_codec.clone(),
        audio_bit_rate: 192_000,
        audio_dup,
        control: true,
        stay_awake: opts.stay_awake,
        power_off_on_close: false,
    };

    let engine = MirrorEngine::new().map_err(|e| format!("Screen Mirror init failed: {e}"))?;
    let session = engine
        .start_mirroring(target_serial.as_deref().filter(|s| !s.is_empty()), Some(params))
        .await
        .map_err(|e| format!("Screen Mirror failed: {e}"))?;

    let handle = super::mirror::launch_mirror(
        app,
        session,
        opts.turn_screen_off,
        audio_buf,
        u16::from(max_fps),
    )
    .await?;

    *MIRROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    *MIRROR_META.lock().unwrap_or_else(|e| e.into_inner()) = Some(ActiveMirrorMeta {
        serial: target_serial,
        max_size,
        max_fps,
        bitrate_mbps,
        audio_enabled,
        audio_buffer_ms: audio_buf,
        audio_codec,
        audio_dup,
        turn_screen_off: opts.turn_screen_off,
    });
    Ok("started".into())
}

/// Apply mirror setting changes in real time while mirroring is active:
/// - `turn_screen_off`: toggles phone display power immediately (`SetDisplayPower`) without restarting.
/// - `audio_enabled`: instant 0ms mute/unmute when audio stream is open (restarts only if audio was initially disabled).
/// - `audio_dup` / stream codec / bitrate / res / fps: hot-restarts active mirror cleanly.
#[tauri::command]
pub async fn sc_live_update(app: tauri::AppHandle, mut opts: StartOpts) -> Result<(), String> {
    if !sc_active() {
        return Ok(());
    }

    let handle = { MIRROR.lock().unwrap_or_else(|e| e.into_inner()).clone() };
    let prev = { MIRROR_META.lock().unwrap_or_else(|e| e.into_inner()).clone() };
    let (Some(handle), Some(prev)) = (handle, prev) else {
        return Ok(());
    };

    let (max_size, max_fps, bitrate_mbps, audio_buf, audio_codec, audio_enabled, audio_dup) =
        sanitize_opts(&opts);

    if opts.turn_screen_off != prev.turn_screen_off {
        handle.set_screen_power(!opts.turn_screen_off);
        if let Ok(mut meta_guard) = MIRROR_META.lock() {
            if let Some(m) = meta_guard.as_mut() {
                m.turn_screen_off = opts.turn_screen_off;
            }
        }
    }

    let needs_restart = max_size != prev.max_size
        || max_fps != prev.max_fps
        || bitrate_mbps != prev.bitrate_mbps
        || audio_buf != prev.audio_buffer_ms
        || audio_codec != prev.audio_codec
        || audio_dup != prev.audio_dup
        || (audio_enabled && !prev.audio_enabled);

    if needs_restart {
        if opts.serial.as_deref().unwrap_or("").is_empty() {
            opts.serial = prev.serial.clone();
        }
        sc_start(app, opts).await.map(|_| ())
    } else {
        if audio_enabled != prev.audio_enabled {
            handle.set_audio_muted(!audio_enabled);
            if let Ok(mut meta_guard) = MIRROR_META.lock() {
                if let Some(m) = meta_guard.as_mut() {
                    m.audio_enabled = audio_enabled;
                }
            }
        }
        Ok(())
    }
}

#[tauri::command]
pub fn sc_stop() {
    if let Some(handle) = MIRROR.lock().unwrap_or_else(|e| e.into_inner()).take() {
        handle.stop();
    }
    if let Ok(mut m) = MIRROR_META.lock() {
        *m = None;
    }
}

#[tauri::command]
pub fn sc_active() -> bool {
    if !is_enabled() {
        sc_stop();
        return false;
    }
    let mut lock = MIRROR.lock().unwrap_or_else(|e| e.into_inner());
    match lock.as_ref().map(|h| h.is_running()) {
        Some(true) => true,
        Some(false) => {
            *lock = None;
            false
        }
        None => false,
    }
}

// ── QR pairing (Wireless debugging → Pair device with QR code) ───────────────────────────────
fn qr_set(id: u64, state: &str, msg: &str) {
    let mut g = QR_STATE.lock().unwrap();
    if QR_GEN.load(Ordering::SeqCst) == id { *g = (id, state.to_string(), msg.to_string()); }
}
fn qr_live(id: u64) -> bool { is_enabled() && QR_GEN.load(Ordering::SeqCst) == id }

async fn adb_out(adb: &AdbClient, args: &[&str]) -> String {
    match adb.new_command().args(args).output().await {
        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(e) => e.to_string(),
    }
}

/// Finds `<service> <type> <ip:port>` rows in `adb mdns services` output.
fn mdns_rows(text: &str, kind: &str) -> Vec<(String, String)> {
    let clean_kind = kind.trim_end_matches('.');
    text.lines().filter_map(|l| {
        let p: Vec<&str> = l.split_whitespace().collect();
        if p.len() >= 3 && p[1].trim_end_matches('.') == clean_kind {
            Some((p[0].to_string(), p[2].to_string()))
        } else {
            None
        }
    }).collect()
}

async fn qr_task(id: u64, name: String, pw: String) {
    let adb = match adb_client() { Ok(a) => a, Err(e) => return qr_set(id, "error", &e) };
    qr_set(id, "waiting", "Scan the QR code on your phone");
    let deadline = Instant::now() + Duration::from_secs(120);
    let pair_addr = loop {
        if !qr_live(id) { return; }
        if Instant::now() > deadline { return qr_set(id, "error", "QR timed out. Try again."); }
        let t = adb_out(&adb, &["mdns", "services"]).await;
        let rows = mdns_rows(&t, "_adb-tls-pairing._tcp");
        if let Some((_, addr)) = rows.into_iter().find(|(n, _)| *n == name || n.contains(&name) || name.contains(n)) {
            break addr;
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    };
    qr_set(id, "pairing", "Phone detected. Pairing…");
    let out = adb_out(&adb, &["pair", &pair_addr, &pw]).await;
    if !out.contains("Successfully paired") { return qr_set(id, "error", out.trim()); }
    let ip = pair_addr.split(':').next().unwrap_or("").to_string();
    qr_set(id, "pairing", "Paired! Connecting to phone…");

    // Fast path: attempt direct connection to port 5555
    if !ip.is_empty() {
        let r5555 = adb_out(&adb, &["connect", &format!("{ip}:5555")]).await;
        if r5555.contains("connected to") || r5555.contains("already connected") {
            let cfg = WirelessConfig {
                ip: ip.clone(),
                port: 5555,
                model: "Paired Phone".to_string(),
                auto_reconnect: true,
                last_connected_at: format!("{:?}", std::time::SystemTime::now()),
            };
            save_wireless_config(&cfg);
            return qr_set(id, "done", &format!("{ip}:5555"));
        }
    }

    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if !qr_live(id) { return; }
        let t = adb_out(&adb, &["mdns", "services"]).await;
        for (_, addr) in mdns_rows(&t, "_adb-tls-connect._tcp") {
            if addr.starts_with(&format!("{ip}:")) || ip.is_empty() {
                let r = adb_out(&adb, &["connect", &addr]).await;
                if r.contains("connected to") || r.contains("already connected") {
                    if let Some((conn_ip, port)) = addr.split_once(':') {
                        if let Ok(p) = port.parse::<u16>() {
                            let cfg = WirelessConfig {
                                ip: conn_ip.to_string(),
                                port: p,
                                model: "Paired Phone".to_string(),
                                auto_reconnect: true,
                                last_connected_at: format!("{:?}", std::time::SystemTime::now()),
                            };
                            save_wireless_config(&cfg);
                        }
                    }
                    return qr_set(id, "done", &addr);
                }
            }
        }
        if !ip.is_empty() {
            let r = adb_out(&adb, &["connect", &format!("{ip}:5555")]).await;
            if r.contains("connected to") || r.contains("already connected") {
                let cfg = WirelessConfig {
                    ip: ip.clone(),
                    port: 5555,
                    model: "Paired Phone".to_string(),
                    auto_reconnect: true,
                    last_connected_at: format!("{:?}", std::time::SystemTime::now()),
                };
                save_wireless_config(&cfg);
                return qr_set(id, "done", &format!("{ip}:5555"));
            }
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
    qr_set(id, "error", "Paired, but the phone did not appear. Keep Wireless debugging on and try Connect.");
}

#[tauri::command]
pub fn sc_qr_start() -> Result<String, String> {
    ensure_enabled()?;
    let id = QR_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let u = uuid::Uuid::new_v4();
    let name = format!("nexus-{}", &u.simple().to_string()[..6]);
    let pw = format!("{:06}", u.as_u128() % 1_000_000);
    let data = format!("WIFI:T:ADB;S:{name};P:{pw};;");
    let svg = qrcode::QrCode::new(data.as_bytes()).map_err(|e| e.to_string())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(160, 160).quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    *QR_STATE.lock().unwrap() = (id, "waiting".into(), "Scan the QR code on your phone".into());
    tauri::async_runtime::spawn(qr_task(id, name, pw));
    Ok(svg)
}

/// Returns [state, message]. state: waiting | pairing | done (message = ip:port) | error | idle
#[tauri::command]
pub fn sc_qr_status() -> (String, String) {
    if !is_enabled() {
        return ("idle".into(), String::new());
    }
    let g = QR_STATE.lock().unwrap();
    if g.1.is_empty() { ("idle".into(), String::new()) } else { (g.1.clone(), g.2.clone()) }
}

#[tauri::command]
pub fn sc_qr_cancel() {
    QR_GEN.fetch_add(1, Ordering::SeqCst);
    *QR_STATE.lock().unwrap() = (0, String::new(), String::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AuraSettings, ModuleState};

    #[test]
    fn disabled_scrcpy_gates_backend_and_resets_state() {
        let mut s = AuraSettings::default();
        s.modules.insert("scrcpy".to_string(), ModuleState { enabled: false });
        sync_with_settings(&s);

        assert!(!is_enabled());
        assert!(!sc_active());
        assert_eq!(sc_qr_status(), ("idle".to_string(), String::new()));
        assert!(sc_qr_start().is_err());

        tauri::async_runtime::block_on(async {
            assert!(sc_devices().await.unwrap().is_empty());
            assert!(sc_saved().await.unwrap().is_none());
            assert!(sc_connect("127.0.0.1".into(), 5555).await.is_err());
            assert!(sc_make_permanent("USB123".into()).await.is_err());
        });

        s.modules.insert("scrcpy".to_string(), ModuleState { enabled: true });
        sync_with_settings(&s);
        assert!(is_enabled());

        s.modules.insert("scrcpy".to_string(), ModuleState { enabled: false });
        sync_with_settings(&s);
        assert!(!is_enabled());
    }
}
