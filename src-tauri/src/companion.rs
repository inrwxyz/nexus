//! Phone Link (Android companion) HTTP API.
//!
//! Everything under /api/companion/* is handled here. server.rs only forwards the request.
//! Commands go straight to executor.rs with an in-memory app list (no disk scan per command).

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::app_launcher::{scan_apps, App};
use crate::settings::{save_settings, AuraSettings};
use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, IP_ADAPTER_INFO};

const UDP_PORT: u16 = 41781;
const HTTP_PORT: u16 = 41780;
/// App list is refreshed in the background after this long. Commands never wait for a rescan.
const APP_CACHE_TTL: Duration = Duration::from_secs(600);

static APP_CACHE: Mutex<Option<(Instant, Vec<App>)>> = Mutex::new(None);
static REFRESHING: AtomicBool = AtomicBool::new(false);

// ───────────────────────── app list cache ─────────────────────────

/// Call once at startup so the first phone command is already fast.
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = cached_apps();
    });
}

fn cached_apps() -> Vec<App> {
    let mut g = APP_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let (empty, stale) = match &*g {
        None => (true, false),
        Some((t, _)) => (false, t.elapsed() > APP_CACHE_TTL),
    };
    if empty {
        // very first call: one scan, then it stays in memory
        *g = Some((Instant::now(), scan_apps()));
    } else if stale && !REFRESHING.swap(true, Ordering::SeqCst) {
        // stale: keep serving the old list, refresh quietly in the background
        std::thread::spawn(|| {
            let fresh = scan_apps();
            *APP_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), fresh));
            REFRESHING.store(false, Ordering::SeqCst);
        });
    }
    g.as_ref().map(|(_, a)| a.clone()).unwrap_or_default()
}

/// RAII: ambiguous window/app matches auto-pick the best one instead of prompting on a console.
struct AutoPick;
impl AutoPick {
    fn on() -> Self {
        crate::window_mgr::AUTO_PICK.with(|f| f.set(true));
        AutoPick
    }
}
impl Drop for AutoPick {
    fn drop(&mut self) {
        crate::window_mgr::AUTO_PICK.with(|f| f.set(false));
    }
}

fn run_command(cmd: &str) -> bool {
    let vocab = crate::vocab::load_vocab();
    let mut aliases = crate::vocab::load_aliases();
    let mut apps = cached_apps();
    let _guard = AutoPick::on();
    crate::executor::execute_command(cmd, &vocab, &mut apps, &mut aliases, None)
}

// ───────────────────────── pairing key / QR ─────────────────────────

/// One-time pairing key (128-bit random). Lives in `companion.pairing_code`.
pub fn new_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn key_is_valid(k: &str) -> bool {
    // old 6-digit PINs are treated as invalid and replaced
    k.len() >= 16
}

fn ensure_key(s: &mut AuraSettings) {
    if s.companion.auth_token.is_some() {
        return;
    }
    let ok = s.companion.pairing_code.as_deref().map(key_is_valid).unwrap_or(false);
    if !ok {
        s.companion.pairing_code = Some(new_key());
        save_settings(s);
    }
}

pub fn local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

/// MAC of the adapter that owns `local_ip()`, as "AA:BB:CC:DD:EE:FF".
pub fn local_mac() -> Option<String> {
    let ip = local_ip();
    unsafe {
        let mut size = 0u32;
        let _ = GetAdaptersInfo(None, &mut size); // ERROR_BUFFER_OVERFLOW, fills `size`
        let mut buf = vec![0u8; size as usize];
        let head = buf.as_mut_ptr() as *mut IP_ADAPTER_INFO;
        if GetAdaptersInfo(Some(head), &mut size) != 0 {
            return None;
        }
        let mut cur = head as *const IP_ADAPTER_INFO;
        while !cur.is_null() {
            let a = &*cur;
            let s: String = a.IpAddressList.IpAddress.String.iter()
                .take_while(|&&c| c != 0).map(|&c| c as u8 as char).collect();
            if s == ip && a.AddressLength == 6 {
                return Some(a.Address[..6].iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(":"));
            }
            cur = a.Next;
        }
    }
    None
}

fn authed(request: &str, state: &Arc<Mutex<AuraSettings>>) -> bool {
    let tok = bearer_token(request).unwrap_or_default();
    let s = state.lock().unwrap();
    token_matches(&s, &tok)
}

/// QR payload: nexus://pair?ip=<PC_IP>&port=41780&key=<KEY>&udp=41781
fn qr_svg(ip: &str, key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let data = format!(
        "nexus://pair?ip={}&port={}&key={}&udp={}",
        ip, HTTP_PORT, key, UDP_PORT
    );
    match qrcode::QrCode::new(data.as_bytes()) {
        Ok(c) => c
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

/// Used by the settings UI (Tauri command and local HTTP status). Only what the UI needs.
pub fn pairing_info(s: &mut AuraSettings) -> serde_json::Value {
    ensure_key(s);
    let paired = s.companion.auth_token.is_some();
    let qr = if paired {
        String::new()
    } else {
        qr_svg(&local_ip(), s.companion.pairing_code.as_deref().unwrap_or(""))
    };
    serde_json::json!({
        "paired": paired,
        "device": s.companion.paired_device,
        "qr_svg": qr,
    })
}

/// Revoke the phone's session and issue a fresh key.
pub fn revoke(s: &mut AuraSettings) {
    s.companion.auth_token = None;
    s.companion.paired_device = None;
    s.companion.pairing_code = Some(new_key());
    save_settings(s);
}

// ───────────────────────── HTTP helpers ─────────────────────────

fn respond(stream: &mut TcpStream, code: u16, text: &str, body: serde_json::Value) {
    let b = body.to_string();
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        text,
        b.len(),
        b
    );
    let _ = stream.write_all(resp.as_bytes());
}

fn err(stream: &mut TcpStream, code: u16, text: &str, msg: &str) {
    respond(stream, code, text, serde_json::json!({ "status": "error", "message": msg }));
}

fn body_of(request: &str) -> &str {
    request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("")
}

fn bearer_token(request: &str) -> Option<String> {
    let head = request.split("\r\n\r\n").next().unwrap_or(request);
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("authorization") {
                let v = v.trim();
                let is_bearer = v.get(..7).map(|p| p.eq_ignore_ascii_case("bearer ")).unwrap_or(false);
                if is_bearer {
                    return Some(v[7..].trim().to_string());
                }
            }
        }
    }
    None
}

fn token_matches(s: &AuraSettings, tok: &str) -> bool {
    !tok.is_empty() && s.companion.auth_token.as_deref() == Some(tok)
}

// ───────────────────────── routes ─────────────────────────

/// Returns true if the request was a /api/companion/* request (and has been answered).
/// `from_local` = the TCP peer is this PC (settings UI); LAN peers get far less.
pub fn handle(
    request: &str,
    stream: &mut TcpStream,
    state: &Arc<Mutex<AuraSettings>>,
    from_local: bool,
) -> bool {
    let first = request.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if !path.starts_with("/api/companion/") {
        return false;
    }
    let route = path.split('?').next().unwrap_or(path);

    match (method, route) {
        ("GET", "/api/companion/status") => status(request, stream, state, from_local),
        ("POST", "/api/companion/pair") => pair(request, stream, state),
        ("POST", "/api/companion/unpair") => unpair(request, stream, state, from_local),
        ("POST", "/api/companion/command") | ("POST", "/api/companion/voice") => {
            command(request, stream, state)
        }
        ("POST", "/api/companion/generate_code") => generate_code(stream, state, from_local),
        ("GET", "/api/companion/audio/devices") => audio_devices(request, stream, state),
        ("POST", "/api/companion/audio/switch") => audio_switch(request, stream, state),
        ("GET", "/api/companion/audio/sessions") => audio_sessions(request, stream, state),
        ("POST", "/api/companion/audio/stream") => audio_stream_ctl(request, stream, state),
        _ => err(stream, 404, "Not Found", "Unknown companion endpoint"),
    }
    true
}

fn generate_code(stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>, from_local: bool) {
    if !from_local {
        return err(stream, 403, "Forbidden", "Local only");
    }
    let mut s = state.lock().unwrap();
    if s.companion.auth_token.is_none() {
        s.companion.pairing_code = Some(new_key());
        save_settings(&mut s);
    }
    drop(s);
    respond(stream, 200, "OK", serde_json::json!({ "status": "ok" }));
}

fn status(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>, from_local: bool) {
    let tok = bearer_token(request);
    let mut s = state.lock().unwrap();

    if let Some(t) = tok {
        // Phone health check: one request when the app opens.
        if !token_matches(&s, &t) {
            drop(s);
            return err(stream, 401, "Unauthorized", "Unauthorized");
        }
        let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string());
        let body = serde_json::json!({
            "status": "ok",
            "paired": true,
            "device": s.companion.paired_device,
            "pc_name": pc_name,
            "voice_relay": s.companion.voice_relay,
            "touchpad": s.companion.touchpad,
            "udp_port": UDP_PORT,
            "mac": local_mac(),
        });
        drop(s);
        return respond(stream, 200, "OK", body);
    }

    if from_local {
        let info = pairing_info(&mut s);
        drop(s);
        return respond(stream, 200, "OK", info);
    }

    // Anonymous LAN request: nothing sensitive
    let paired = s.companion.auth_token.is_some();
    drop(s);
    respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "paired": paired }))
}

fn pair(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    #[derive(serde::Deserialize)]
    struct PairReq {
        key: Option<String>,
        code: Option<String>, // old field name, same meaning
        device: Option<String>,
    }
    let req: PairReq = match serde_json::from_str(body_of(request)) {
        Ok(r) => r,
        Err(_) => return err(stream, 400, "Bad Request", "Malformed pairing request"),
    };
    let given = req.key.or(req.code).unwrap_or_default();

    let mut s = state.lock().unwrap();
    if s.companion.auth_token.is_some() {
        drop(s);
        return err(stream, 409, "Conflict", "This PC is already paired. Unpair it first.");
    }
    let expected = s.companion.pairing_code.clone().unwrap_or_default();
    if !key_is_valid(&expected) || given.trim() != expected {
        drop(s);
        return err(stream, 403, "Forbidden", "Invalid or expired QR code. Reopen Phone Link on the PC.");
    }

    let token = uuid::Uuid::new_v4().to_string();
    let device = req.device.unwrap_or_else(|| "Android Device".to_string());
    let pc_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string());
    s.companion.auth_token = Some(token.clone());
    s.companion.paired_device = Some(device.clone());
    s.companion.pairing_code = None; // key is one-time
    save_settings(&mut s);
    drop(s);

    respond(
        stream,
        200,
        "OK",
        serde_json::json!({ "status": "ok", "token": token, "pc_name": pc_name, "device": device, "udp_port": UDP_PORT, "mac": local_mac() }),
    );
}

fn unpair(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>, from_local: bool) {
    let tok = bearer_token(request).unwrap_or_default();
    let mut s = state.lock().unwrap();
    if !from_local && !token_matches(&s, &tok) {
        drop(s);
        return err(stream, 401, "Unauthorized", "Unauthorized");
    }
    revoke(&mut s);
    drop(s);
    respond(stream, 200, "OK", serde_json::json!({ "status": "ok" }));
}

fn command(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    #[derive(serde::Deserialize)]
    struct CmdReq {
        token: Option<String>,
        command: Option<String>,
        query: Option<String>, // alias used by /voice
    }
    let req: CmdReq = match serde_json::from_str(body_of(request)) {
        Ok(r) => r,
        Err(_) => return err(stream, 400, "Bad Request", "Malformed command payload"),
    };
    let tok = bearer_token(request).or(req.token).unwrap_or_default();

    let (token_ok, allowed, island) = {
        let s = state.lock().unwrap();
        (
            token_matches(&s, &tok),
            s.module_enabled("companion") && s.companion.voice_relay,
            s.show_on_island && s.companion.show_on_island,
        )
    };
    if !token_ok {
        return err(stream, 401, "Unauthorized", "Unauthorized");
    }
    if !allowed {
        return err(stream, 403, "Forbidden", "Voice relay is disabled in Nexus settings");
    }

    let cmd = req.command.or(req.query).unwrap_or_default().trim().to_string();
    if cmd.is_empty() {
        return err(stream, 400, "Bad Request", "Empty command");
    }

    if island {
        crate::island_bridge::notify_thinking(Some(&cmd));
    }
    let success = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_command(&cmd))).unwrap_or(false);
    if island {
        crate::island_bridge::notify_done(success);
    }

    respond(
        stream,
        200,
        "OK",
        serde_json::json!({ "status": "ok", "command": cmd, "success": success }),
    );
}

fn audio_devices(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    if !authed(request, state) { return err(stream, 401, "Unauthorized", "Unauthorized"); }
    let devices: Vec<_> = crate::audio_switch::list().into_iter().map(|d| serde_json::json!({
        "id": d.id, "name": d.name, "kind": d.kind, "default": d.is_default
    })).collect();
    respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "devices": devices }));
}

fn audio_switch(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    if !authed(request, state) { return err(stream, 401, "Unauthorized", "Unauthorized"); }
    #[derive(serde::Deserialize)]
    struct Req { id: Option<String>, kind: Option<String> }
    let r: Req = match serde_json::from_str(body_of(request)) {
        Ok(r) => r,
        Err(_) => return err(stream, 400, "Bad Request", "Malformed payload"),
    };
    let target = r.id.or(r.kind).unwrap_or_default();
    let ok = std::panic::catch_unwind(|| crate::audio_switch::switch(&target)).unwrap_or(false);
    respond(stream, 200, "OK", serde_json::json!({ "status": if ok { "ok" } else { "error" }, "success": ok }));
}

fn audio_sessions(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    if !authed(request, state) { return err(stream, 401, "Unauthorized", "Unauthorized"); }
    let v: Vec<_> = crate::audio_native::sessions().into_iter()
        .map(|(n, vol, m)| serde_json::json!({ "app": n, "volume": vol, "muted": m })).collect();
    respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "sessions": v }));
}

/// POST {"action":"start","port":41782} | {"action":"stop"}. Audio goes to the caller's IP.
fn audio_stream_ctl(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    if !authed(request, state) { return err(stream, 401, "Unauthorized", "Unauthorized"); }
    #[derive(serde::Deserialize)]
    struct Req { action: String, port: Option<u16> }
    let r: Req = match serde_json::from_str(body_of(request)) {
        Ok(r) => r,
        Err(_) => return err(stream, 400, "Bad Request", "Malformed payload"),
    };
    if r.action == "stop" {
        crate::audio_stream::stop();
        return respond(stream, 200, "OK", serde_json::json!({ "status": "ok" }));
    }
    let peer = match stream.peer_addr() {
        Ok(p) => p.ip(),
        Err(_) => return err(stream, 500, "Error", "No peer"),
    };
    let dst = std::net::SocketAddr::new(peer, r.port.unwrap_or(41782));
    match crate::audio_stream::start(dst) {
        Ok(rate) => respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "rate": rate, "channels": 2 })),
        Err(e) => err(stream, 500, "Error", &e),
    }
}
