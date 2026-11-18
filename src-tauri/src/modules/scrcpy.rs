//! Scrcpy module: phone screen mirroring over USB, QR pairing, or IP + port.
//! Engine = the `rust-scrcpy` crate (ADB + permanent wireless). The mirror window is the bundled
//! scrcpy.exe. Nothing runs and no RAM is used until a command below is called.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rust_scrcpy::{AdbClient, DeviceInfo, WirelessConfig, WirelessManager};
use serde::Deserialize;

static MIRROR: Mutex<Option<Child>> = Mutex::new(None);
static ADB: Mutex<Option<AdbClient>> = Mutex::new(None);
static LAST_AUTO_SAVED: Mutex<Option<(String, u16)>> = Mutex::new(None);
static FORGOTTEN: AtomicBool = AtomicBool::new(false);
static QR_GEN: AtomicU64 = AtomicU64::new(0);
static QR_STATE: Mutex<(u64, String, String)> = Mutex::new((0, String::new(), String::new()));

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

fn find_asset(name: &str) -> Option<PathBuf> {
    let base = exe_dir();
    let subs = ["", "assets", "resources", "resources/assets", "_up_/assets", "resources/_up_/assets"];
    for a in base.ancestors().take(5) {
        for s in subs {
            let p = a.join(s).join(name);
            if p.is_file() { return Some(p); }
        }
    }
    let rel = [format!("assets/{name}"), format!("../assets/{name}"), name.to_string()];
    rel.into_iter().map(PathBuf::from).find(|p| p.is_file())
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
    let c = WirelessManager::new(adb_client()?).setup_permanent_wireless(&serial).await.map_err(|e| e.to_string())?;
    save_wireless_config(&c);
    Ok(c)
}

#[tauri::command]
pub async fn sc_saved() -> Result<Option<WirelessConfig>, String> {
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOpts {
    serial: Option<String>,
    max_size: u16,
    max_fps: u16,
    bitrate_mbps: u32,
    stay_awake: bool,
    turn_screen_off: bool,
    audio_buffer_ms: Option<u16>,
    audio_codec: Option<String>,
    audio_dup: Option<bool>,
}

#[tauri::command]
pub async fn sc_start(opts: StartOpts) -> Result<String, String> {
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
    let exe = find_asset("scrcpy.exe").ok_or("scrcpy.exe not found in Nexus assets")?;
    let mut cmd = Command::new(&exe);

    // Pass resolved ADB executable so scrcpy never locks or conflicts with bundled assets/adb.exe
    if let Ok(adb) = adb_client() {
        cmd.env("ADB", adb.get_adb_path());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());

    if let Some(s) = target_serial.as_deref().filter(|s| !s.is_empty()) {
        cmd.args(["-s", s]);
    }

    // Defensive parameter sanitization & clamping:
    // Prevents malformed frontend values from causing scrcpy exit code 1.
    let max_size = match opts.max_size { 1280 => 1280, 1920 => 1920, _ => 0 };
    if max_size > 0 {
        cmd.args(["-m", &max_size.to_string()]);
    }

    let max_fps = if opts.max_fps >= 100 { 120 } else { 60 };
    cmd.args(["--max-fps", &max_fps.to_string()]);

    let bitrate = match opts.bitrate_mbps { 4 => 4, 16 => 16, _ => 8 };
    cmd.args(["-b", &format!("{}M", bitrate)]);

    let audio_buf = match opts.audio_buffer_ms.unwrap_or(120) { 40 => 40, 250 => 250, _ => 120 };
    cmd.args(["--audio-buffer", &audio_buf.to_string()]);
    cmd.args(["--audio-output-buffer", "20", "--video-buffer", "25", "--audio-bit-rate", "192K"]);

    let codec = match opts.audio_codec.as_deref() {
        Some("opus") => Some("opus"),
        Some("aac") => Some("aac"),
        Some("flac") => Some("flac"),
        Some("raw") => Some("raw"),
        _ => None, // "auto" or anything else -> let scrcpy use default (opus)
    };
    if let Some(c) = codec {
        cmd.args(["--audio-codec", c]);
    }

    if opts.audio_dup.unwrap_or(false) { cmd.arg("--audio-dup"); }
    if opts.stay_awake { cmd.arg("--stay-awake"); }
    if opts.turn_screen_off { cmd.arg("--turn-screen-off"); }
    cmd.args(["--window-title", "Nexus Screen Mirror"]);

    let mut child = cmd.spawn().map_err(|e| format!("Failed to launch scrcpy: {e}"))?;

    // Early-exit watch: if scrcpy dies right away (unsupported audio codec on Android < 11,
    // unsupported fps, device dropped, ...) report why instead of silently going back to "Start".
    let deadline = Instant::now() + Duration::from_millis(700);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut raw = Vec::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_end(&mut raw);
                }
                return Err(early_exit_msg(&String::from_utf8_lossy(&raw), status.code()));
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if Instant::now() >= deadline { break; }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Still running: keep draining stderr so a full pipe can never stall the mirror.
    if let Some(mut e) = child.stderr.take() {
        std::thread::spawn(move || { let _ = std::io::copy(&mut e, &mut std::io::sink()); });
    }
    *MIRROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok("started".into())
}

/// Turns scrcpy's stderr into one short, readable line for the UI.
fn early_exit_msg(stderr: &str, code: Option<i32>) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let errs: Vec<&str> = lines.iter().copied().filter(|l| l.contains("ERROR")).collect();
    let src = if errs.is_empty() { &lines } else { &errs };
    let tail: Vec<&str> = src.iter().rev().take(2).rev().copied().collect();
    if tail.is_empty() {
        return format!("Scrcpy exited immediately (code {})", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()));
    }
    let joined = tail.join(" · ").replace("ERROR: ", "");
    let short: String = joined.chars().take(240).collect();
    format!("Scrcpy failed: {short}")
}


#[tauri::command]
pub fn sc_stop() {
    if let Some(mut c) = MIRROR.lock().unwrap_or_else(|e| e.into_inner()).take() { let _ = c.kill(); }
}

#[tauri::command]
pub fn sc_active() -> bool {
    let mut lock = MIRROR.lock().unwrap();
    match lock.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => { *lock = None; false }
        None => false,
    }
}

// ── QR pairing (Wireless debugging → Pair device with QR code) ───────────────────────────────
fn qr_set(id: u64, state: &str, msg: &str) {
    let mut g = QR_STATE.lock().unwrap();
    if QR_GEN.load(Ordering::SeqCst) == id { *g = (id, state.to_string(), msg.to_string()); }
}
fn qr_live(id: u64) -> bool { QR_GEN.load(Ordering::SeqCst) == id }

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
    let id = QR_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let u = uuid::Uuid::new_v4();
    let name = format!("nexus-{}", &u.simple().to_string()[..6]);
    let pw = format!("{:06}", u.as_u128() % 1_000_000);
    let data = format!("WIFI:T:ADB;S:{name};P:{pw};;");
    let svg = qrcode::QrCode::new(data.as_bytes()).map_err(|e| e.to_string())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200).quiet_zone(true)
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
    let g = QR_STATE.lock().unwrap();
    if g.1.is_empty() { ("idle".into(), String::new()) } else { (g.1.clone(), g.2.clone()) }
}

#[tauri::command]
pub fn sc_qr_cancel() {
    QR_GEN.fetch_add(1, Ordering::SeqCst);
    *QR_STATE.lock().unwrap() = (0, String::new(), String::new());
}
