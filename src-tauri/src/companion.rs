//! Phone Link (Android companion) HTTP API.
//!
//! Everything under /api/companion/* is handled here. server.rs only forwards the request.
//! Commands go straight to executor.rs with an in-memory app list (no disk scan per command).

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::app_launcher::{scan_apps, App};
use crate::settings::{save_settings, AuraSettings};
use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, IP_ADAPTER_INFO};

const UDP_PORT: u16 = 41781;
const HTTP_PORT: u16 = 41780;
/// App list is refreshed in the background after this long. Commands never wait for a rescan.
const APP_CACHE_TTL: Duration = Duration::from_secs(600);

static APP_CACHE: Mutex<Option<(Instant, Vec<App>)>> = Mutex::new(None);
static REFRESHING: AtomicBool = AtomicBool::new(false);
static LAST_PHONE_IP: Mutex<Option<String>> = Mutex::new(None);

fn app_handle_slot() -> &'static Mutex<Option<AppHandle>> {
    static APP: OnceLock<Mutex<Option<AppHandle>>> = OnceLock::new();
    APP.get_or_init(|| Mutex::new(None))
}

pub fn set_app_handle(app: AppHandle) {
    *app_handle_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(app);
}

pub fn emit_pairing_changed(s: &AuraSettings) {
    if let Some(app) = app_handle_slot().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = app.emit(
            "companion-pairing-changed",
            serde_json::json!({
                "paired": s.companion.auth_token.is_some(),
                "device": s.companion.paired_device,
                "gamepad_enabled": crate::gamepad::is_enabled(),
                "remote_terminal": s.module_enabled("companion") && s.companion.remote_terminal,
            }),
        );
    }
}

pub fn get_last_phone_ip() -> Option<String> {
    LAST_PHONE_IP.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

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
            .min_dimensions(180, 180)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#000000"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
        Err(_) => String::new(),
    }
}

/// Used by the settings UI to check pairing state without pre-generating a QR code.
pub fn pairing_info(s: &AuraSettings) -> serde_json::Value {
    let paired = s.companion.auth_token.is_some();
    serde_json::json!({
        "paired": paired,
        "device": s.companion.paired_device,
    })
}

/// Generates a fresh one-time pairing code + QR SVG on explicit user click.
pub fn generate_pairing_qr(s: &mut AuraSettings) -> serde_json::Value {
    if s.companion.auth_token.is_some() {
        return pairing_info(s);
    }
    let key = new_key();
    s.companion.pairing_code = Some(key.clone());
    save_settings(s);
    serde_json::json!({
        "paired": false,
        "qr_svg": qr_svg(&local_ip(), &key),
    })
}

/// Revoke the phone's session, push instant UDP disconnect to phone, suspend gamepad, and notify PC UI.
pub fn revoke(s: &mut AuraSettings) {
    let _ = crate::trackpad::push_udp_to_phone("{\"type\":\"unauthorized\"}");
    s.companion.auth_token = None;
    s.companion.paired_device = None;
    s.companion.pairing_code = Some(new_key());
    save_settings(s);
    crate::gamepad::sync_with_settings(s);
    emit_pairing_changed(s);
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

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let head = head.split("\r\n\r\n").next().unwrap_or(head);
    for line in head.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

fn bearer_token(request: &str) -> Option<String> {
    let v = header_value(request, "authorization")?;
    let is_bearer = v
        .get(..7)
        .map(|p| p.eq_ignore_ascii_case("bearer "))
        .unwrap_or(false);
    if is_bearer {
        Some(v[7..].trim().to_string())
    } else {
        None
    }
}

fn token_matches(s: &AuraSettings, tok: &str) -> bool {
    !tok.is_empty() && s.companion.auth_token.as_deref() == Some(tok)
}

// ───────────────────────── routes ─────────────────────────

/// Returns true if the request was a /api/companion/* request (and has been answered).
/// `from_local` = the TCP peer is this PC (settings UI); LAN peers get far less.
pub fn handle_bytes(
    head: &str,
    body: &[u8],
    stream: &mut TcpStream,
    state: &Arc<Mutex<AuraSettings>>,
    from_local: bool,
) -> bool {
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if !path.starts_with("/api/companion/") {
        return false;
    }
    if let Ok(peer) = stream.peer_addr() {
        let ip = peer.ip().to_string();
        if ip != "127.0.0.1" && !ip.starts_with("::1") {
            *LAST_PHONE_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(ip);
        }
    }
    let route = path.split('?').next().unwrap_or(path);
    let body_str = || String::from_utf8_lossy(body);

    match (method, route) {
        ("GET", "/api/companion/status") => status(head, stream, state, from_local),
        ("POST", "/api/companion/pair") => {
            let full = format!("{head}\r\n\r\n{}", body_str());
            pair(&full, stream, state)
        }
        ("POST", "/api/companion/unpair") => unpair(head, stream, state, from_local),
        ("POST", "/api/companion/command") | ("POST", "/api/companion/voice") => {
            let full = format!("{head}\r\n\r\n{}", body_str());
            command(&full, stream, state)
        }
        ("POST", "/api/companion/clipboard") => clipboard(head, body, stream, state),
        ("POST", "/api/companion/terminal") => crate::remote_terminal::handle_http(head, body, stream, state),
        ("POST", "/api/companion/generate_code") => generate_code(stream, state, from_local),
        ("GET", "/api/companion/audio/devices") => audio_devices(head, stream, state),
        ("POST", "/api/companion/audio/switch") => {
            let full = format!("{head}\r\n\r\n{}", body_str());
            audio_switch(&full, stream, state)
        }
        ("GET", "/api/companion/audio/sessions") => audio_sessions(head, stream, state),
        ("GET", "/api/companion/gamepad/profiles") => gamepad_profiles_get(head, stream, state),
        ("POST", "/api/companion/gamepad/profile") => gamepad_profile_save(head, body, stream, state),
        _ => err(stream, 404, "Not Found", "Unknown companion endpoint"),
    }
    true
}

fn generate_code(stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>, from_local: bool) {
    if !from_local {
        return err(stream, 403, "Forbidden", "Local only");
    }
    let mut s = state.lock().unwrap();
    let payload = generate_pairing_qr(&mut s);
    drop(s);
    respond(stream, 200, "OK", payload);
}

fn status(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>, from_local: bool) {
    let tok = bearer_token(request);
    let s = state.lock().unwrap();

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
            "remote_terminal": s.module_enabled("companion") && s.companion.remote_terminal,
            "gamepad_enabled": s.module_should_run_now("gamepad") && s.module_should_run_now("companion") && s.companion.auth_token.is_some(),
            "gamepad": s.gamepad,
            "udp_port": UDP_PORT,
            "mac": local_mac(),
        });
        drop(s);
        return respond(stream, 200, "OK", body);
    }

    if from_local {
        let info = pairing_info(&s);
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
    crate::gamepad::sync_with_settings(&s);
    emit_pairing_changed(&s);
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

    let is_timer = crate::executor::is_timer_command(&cmd);
    if island && !is_timer {
        crate::island_bridge::notify_thinking(Some(&cmd));
    }
    let started = std::time::Instant::now();
    let success = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_command(&cmd))).unwrap_or(false);
    if island && !is_timer {
        // Give Island's 130ms enter transition time to settle before swapping to done/fail.
        let elapsed = started.elapsed();
        let min_hold = Duration::from_millis(220);
        if elapsed < min_hold {
            std::thread::sleep(min_hold - elapsed);
        }
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

fn clipboard(head: &str, body: &[u8], stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    let tok = bearer_token(head).unwrap_or_default();
    let (token_ok, companion_on) = {
        let s = state.lock().unwrap();
        (token_matches(&s, &tok), s.module_enabled("companion"))
    };
    if !token_ok {
        return err(stream, 401, "Unauthorized", "Unauthorized");
    }
    if !companion_on {
        return err(stream, 403, "Forbidden", "Phone Link module is disabled on PC");
    }

    let ct = header_value(head, "content-type").unwrap_or("application/json");
    let payload = match crate::clipboard_sync::parse_payload(ct, body) {
        Ok(p) => p,
        Err(msg) => return err(stream, 400, "Bad Request", &msg),
    };

    match crate::clipboard_sync::apply_payload(&payload) {
        Ok(kind) => {
            println!("📋 Synced phone clipboard ({kind}, {} bytes)", body.len());
            respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "kind": kind }));
        }
        Err(msg) => err(stream, 500, "Internal Server Error", &msg),
    }
}

fn gamepad_profiles_get(request: &str, stream: &mut TcpStream, state: &Arc<Mutex<AuraSettings>>) {
    let tok = bearer_token(request).unwrap_or_default();
    let (token_ok, allowed, auto_detect) = {
        let s = state.lock().unwrap();
        (
            token_matches(&s, &tok),
            s.module_enabled("companion") && s.module_enabled("gamepad"),
            s.gamepad.auto_detect,
        )
    };
    if !token_ok {
        return err(stream, 401, "Unauthorized", "Unauthorized");
    }
    if !allowed {
        return err(stream, 403, "Forbidden", "Game Controller is disabled on PC");
    }
    let snap = crate::gamepad::profiles::get_store_snapshot();
    respond(
        stream,
        200,
        "OK",
        serde_json::json!({
            "status": "ok",
            "auto_detect": auto_detect,
            "active_profile_id": snap.active_profile_id,
            "detected_game": snap.detected_game,
            "profiles": snap.profiles,
        }),
    );
}

fn gamepad_profile_save(
    head: &str,
    body: &[u8],
    stream: &mut TcpStream,
    state: &Arc<Mutex<AuraSettings>>,
) {
    let tok = bearer_token(head).unwrap_or_default();
    let (token_ok, allowed) = {
        let s = state.lock().unwrap();
        (
            token_matches(&s, &tok),
            s.module_enabled("companion") && s.module_enabled("gamepad"),
        )
    };
    if !token_ok {
        return err(stream, 401, "Unauthorized", "Unauthorized");
    }
    if !allowed {
        return err(stream, 403, "Forbidden", "Game Controller is disabled on PC");
    }

    #[derive(serde::Deserialize)]
    struct SaveReq {
        #[serde(default)]
        make_active: Option<bool>,
        #[serde(default)]
        select_only: Option<bool>,
        #[serde(default)]
        profile_id: Option<String>,
        profile: Option<crate::gamepad::profiles::GamepadProfile>,
    }

    let req: SaveReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return err(stream, 400, "Bad Request", "Malformed gamepad profile payload"),
    };

    if req.select_only.unwrap_or(false) {
        if let Some(pid) = req.profile_id {
            return match crate::gamepad::profiles::select_profile(&pid, false) {
                Ok(snap) => respond(stream, 200, "OK", serde_json::json!({ "status": "ok", "active_profile_id": snap.active_profile_id, "profiles": snap.profiles })),
                Err(msg) => err(stream, 404, "Not Found", &msg),
            };
        }
    }

    let Some(prof) = req.profile else {
        return err(stream, 400, "Bad Request", "Missing profile");
    };
    let snap = crate::gamepad::profiles::upsert_profile(prof, req.make_active.unwrap_or(true), false);
    respond(
        stream,
        200,
        "OK",
        serde_json::json!({
            "status": "ok",
            "active_profile_id": snap.active_profile_id,
            "profiles": snap.profiles,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    fn invoke_companion_http(head: &str, body: &[u8], settings: AuraSettings) -> (bool, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client_handle = std::thread::spawn(move || {
            let mut client = TcpStream::connect(addr).unwrap();
            let mut out = String::new();
            let _ = client.read_to_string(&mut out);
            out
        });

        let (mut server_stream, _) = listener.accept().unwrap();
        let state = Arc::new(Mutex::new(settings));
        let handled = handle_bytes(head, body, &mut server_stream, &state, false);
        drop(server_stream);
        let resp = client_handle.join().unwrap();
        (handled, resp)
    }

    #[test]
    fn clipboard_endpoint_enforces_auth_module_gate_and_validation() {
        let mut s = AuraSettings::default();
        s.companion.auth_token = Some("tok-123".into());

        // 1. Missing/bad token -> 401
        let (ok, resp) = invoke_companion_http(
            "POST /api/companion/clipboard HTTP/1.1\r\nContent-Type: application/json\r\n\r\n",
            br#"{"kind":"text","text":"hello"}"#,
            s.clone(),
        );
        assert!(ok);
        assert!(resp.starts_with("HTTP/1.1 401"));

        // 2. Valid token + companion module off -> 403
        let mut disabled = s.clone();
        disabled.modules.get_mut("companion").unwrap().enabled = false;
        let (ok, resp) = invoke_companion_http(
            "POST /api/companion/clipboard HTTP/1.1\r\nAuthorization: Bearer tok-123\r\nContent-Type: application/json\r\n\r\n",
            br#"{"kind":"text","text":"hello"}"#,
            disabled,
        );
        assert!(ok);
        assert!(resp.starts_with("HTTP/1.1 403"));

        // 3. Valid token + bad PNG -> 400
        let (ok, resp) = invoke_companion_http(
            "POST /api/companion/clipboard HTTP/1.1\r\nAuthorization: Bearer tok-123\r\nContent-Type: image/png\r\n\r\n",
            b"bad-bytes",
            s,
        );
        assert!(ok);
        assert!(resp.starts_with("HTTP/1.1 400"));
    }

    #[test]
    fn terminal_endpoint_is_disabled_by_default_and_checks_token() {
        let mut s = AuraSettings::default();
        s.companion.auth_token = Some("tok-123".into());
        assert!(!s.companion.remote_terminal);

        // Unauthorized -> 401
        let (ok, resp) = invoke_companion_http(
            "POST /api/companion/terminal HTTP/1.1\r\nContent-Type: application/json\r\n\r\n",
            br#"{"command":"echo hi"}"#,
            s.clone(),
        );
        assert!(ok);
        assert!(resp.starts_with("HTTP/1.1 401"));

        // Valid token, default remote_terminal=false -> 403
        let (ok, resp) = invoke_companion_http(
            "POST /api/companion/terminal HTTP/1.1\r\nAuthorization: Bearer tok-123\r\nContent-Type: application/json\r\n\r\n",
            br#"{"command":"echo hi"}"#,
            s,
        );
        assert!(ok);
        assert!(resp.starts_with("HTTP/1.1 403"));
    }
}
