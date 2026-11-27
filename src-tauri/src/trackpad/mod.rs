//! UDP Trackpad & multilingual keyboard input listener for Nexus Phone Companion.

mod win32_input;

use std::net::UdpSocket;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::settings::AuraSettings;
use win32_input::{
    check_and_notify_input_focus, handle_key, handle_text, send_mouse_click, send_mouse_down,
    send_mouse_move, send_mouse_up, send_wheel_scroll, trigger_gesture_by_name, LEFT_HELD,
};

pub use win32_input::press_combo;

pub const UDP_PORT: u16 = 41781;

static RUNNING: AtomicBool = AtomicBool::new(true);
static INERTIA_WAKE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static UDP_SOCKET: OnceLock<UdpSocket> = OnceLock::new();
static LAST_PHONE_ADDR: Mutex<Option<std::net::SocketAddr>> = Mutex::new(None);

/// Sends a JSON control event back to the most recently active authenticated phone over UDP 41781.
pub fn push_udp_to_phone(json: &str) -> bool {
    let Some(sock) = UDP_SOCKET.get() else { return false };
    let addr = *LAST_PHONE_ADDR.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(a) = addr {
        sock.send_to(json.as_bytes(), a).is_ok()
    } else {
        false
    }
}

fn record_phone_udp(src: std::net::SocketAddr) {
    let mut g = LAST_PHONE_ADDR.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref() != Some(&src) {
        *g = Some(src);
    }
}

fn wake_inertia() {
    let _g = INERTIA_WAKE.0.lock().unwrap();
    INERTIA_WAKE.1.notify_one();
}

#[derive(Debug, Deserialize)]
struct JsonPacket {
    #[serde(default)]
    token: String,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    dx: f32,
    #[serde(default)]
    dy: f32,
    #[serde(default)]
    btn: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Default)]
struct TrackpadState {
    acc_x: f32,
    acc_y: f32,
    scroll_acc_y: f32,
    scroll_acc_x: f32,
    last_scroll_time: Option<Instant>,
    scroll_vel_y: f32,
    scroll_vel_x: f32,
    inertia_running: bool,
}

pub fn start_trackpad_listener(settings_state: Arc<Mutex<AuraSettings>>) {
    thread::spawn(move || {
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", UDP_PORT)) {
            Ok(s) => {
                s.set_read_timeout(Some(Duration::from_millis(500))).ok();
                if let Ok(clone) = s.try_clone() {
                    let _ = UDP_SOCKET.set(clone);
                }
                s
            }
            Err(e) => {
                eprintln!("[Nexus Trackpad] Failed to bind UDP {}: {}", UDP_PORT, e);
                return;
            }
        };

        let state = Arc::new(Mutex::new(TrackpadState::default()));
        let mut buffer = [0u8; 4096];

        let inertia_state = state.clone();
        let inertia_settings = settings_state.clone();
        thread::spawn(move || {
            while RUNNING.load(Ordering::Relaxed) {
                {
                    let g = INERTIA_WAKE.0.lock().unwrap();
                    if !inertia_state.lock().unwrap().inertia_running {
                        let _ = INERTIA_WAKE.1.wait_timeout(g, Duration::from_secs(30));
                        continue;
                    }
                }
                thread::sleep(Duration::from_millis(16));
                let (has_inertia, vy, vx) = {
                    let mut s = inertia_state.lock().unwrap();
                    if s.inertia_running {
                        s.scroll_vel_y *= 0.92;
                        s.scroll_vel_x *= 0.92;
                        if s.scroll_vel_y.abs() < 0.5 && s.scroll_vel_x.abs() < 0.5 {
                            s.inertia_running = false;
                            s.scroll_vel_y = 0.0;
                            s.scroll_vel_x = 0.0;
                            (false, 0.0, 0.0)
                        } else {
                            (true, s.scroll_vel_y, s.scroll_vel_x)
                        }
                    } else {
                        (false, 0.0, 0.0)
                    }
                };

                if has_inertia {
                    let settings = inertia_settings.lock().unwrap();
                    if settings.companion.inertia_scrolling {
                        let step_y = (vy * 0.3).round() as i32;
                        let step_x = (vx * 0.3).round() as i32;
                        if step_y != 0 || step_x != 0 {
                            send_wheel_scroll(step_y, step_x);
                        }
                    }
                }
            }
        });

        let mut last_unauth: Option<Instant> = None;
        let mut last_pkt = Instant::now();
        while RUNNING.load(Ordering::Relaxed) {
            let (len, src) = match socket.recv_from(&mut buffer) {
                Ok(res) => {
                    last_pkt = Instant::now();
                    res
                }
                Err(_) => {
                    if LEFT_HELD.load(Ordering::Relaxed) && last_pkt.elapsed() > Duration::from_secs(10) {
                        send_mouse_up(0);
                    }
                    continue;
                }
            };

            if len == 0 {
                continue;
            }

            let (tp_enabled, gp_enabled, sensitivity, gp_cfg, token) = {
                let s = settings_state.lock().unwrap();
                let master = s.module_enabled("companion");
                let tp = s.companion.touchpad;
                let gp = s.module_enabled("gamepad");
                (master && tp, master && gp, s.companion.sensitivity, s.gamepad.clone(), s.companion.auth_token.clone())
            };

            if !tp_enabled && !gp_enabled {
                continue;
            }

            if buffer[0] == b'{' {
                if !tp_enabled {
                    continue;
                }
                if let Ok(pkt) = serde_json::from_slice::<JsonPacket>(&buffer[..len]) {
                    match &token {
                        Some(expected) if &pkt.token == expected => {
                            record_phone_udp(src);
                        }
                        _ => {
                            notify_unauthorized(&socket, src, &mut last_unauth);
                            continue;
                        }
                    }
                    handle_json_event(&pkt, sensitivity, &state, &socket, src);
                }
            } else if len >= 16 {
                let token_hash = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                match &token {
                    Some(expected) if token_hash == crc32fast::hash(expected.as_bytes()) => {
                        record_phone_udp(src);
                    }
                    _ => {
                        notify_unauthorized(&socket, src, &mut last_unauth);
                        continue;
                    }
                }

                if buffer[4] == crate::gamepad::PKT_GAMEPAD {
                    if gp_enabled {
                        crate::gamepad::handle_binary_packet(&buffer[..len], &gp_cfg);
                    }
                    continue;
                }

                if !tp_enabled {
                    continue;
                }
                let event_type = buffer[4];
                let param = buffer[5];
                let flags = u16::from_le_bytes([buffer[6], buffer[7]]);
                let dx = f32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
                let dy = f32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);

                handle_binary_event(event_type, param, flags, dx, dy, sensitivity, &state, &socket, src);
            }
        }
    });
}

fn notify_unauthorized(socket: &UdpSocket, dst: std::net::SocketAddr, last: &mut Option<Instant>) {
    if let Some(t) = last {
        if t.elapsed() < Duration::from_secs(2) {
            return;
        }
    }
    *last = Some(Instant::now());
    let _ = socket.send_to(b"{\"status\":\"unauthorized\"}", dst);
}

fn handle_json_event(
    pkt: &JsonPacket,
    sensitivity: f32,
    state: &Arc<Mutex<TrackpadState>>,
    socket: &UdpSocket,
    src: std::net::SocketAddr,
) {
    match pkt.event_type.as_str() {
        "move" => {
            {
                let mut s = state.lock().unwrap();
                s.inertia_running = false;
            }
            process_subpixel_move(pkt.dx, pkt.dy, sensitivity, state);
        }
        "down" => send_mouse_down(parse_button_str(pkt.btn.as_deref().unwrap_or("left"))),
        "up" => send_mouse_up(parse_button_str(pkt.btn.as_deref().unwrap_or("left"))),
        "click" => {
            let btn = parse_button_str(pkt.btn.as_deref().unwrap_or("left"));
            send_mouse_click(btn);
            if btn == 0 {
                check_and_notify_input_focus(socket, src);
            }
        }
        "scroll" => process_scroll(pkt.dx, pkt.dy, state),
        "scroll_end" => {
            {
                let mut s = state.lock().unwrap();
                s.inertia_running = true;
            }
            wake_inertia();
        }
        "gesture" => {
            if let Some(name) = &pkt.name {
                trigger_gesture_by_name(name);
            }
        }
        "key" => {
            if let Some(k) = &pkt.key {
                handle_key(k);
            }
        }
        "text" => {
            if let Some(t) = &pkt.text {
                handle_text(t);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_binary_event(
    event_type: u8,
    param: u8,
    flags: u16,
    dx: f32,
    dy: f32,
    sensitivity: f32,
    state: &Arc<Mutex<TrackpadState>>,
    socket: &UdpSocket,
    src: std::net::SocketAddr,
) {
    match event_type {
        1 => {
            {
                let mut s = state.lock().unwrap();
                s.inertia_running = false;
            }
            process_subpixel_move(dx, dy, sensitivity, state);
        }
        2 => send_mouse_down(param),
        3 => send_mouse_up(param),
        4 => {
            send_mouse_click(param);
            if param == 0 {
                check_and_notify_input_focus(socket, src);
            }
        }
        5 => {
            process_scroll(dx, dy, state);
            if (flags & 0x01) != 0 {
                {
                    let mut s = state.lock().unwrap();
                    s.inertia_running = true;
                }
                wake_inertia();
            }
        }
        6 => match param {
            1 => trigger_gesture_by_name("swipe_up"),
            2 => trigger_gesture_by_name("swipe_down"),
            3 => trigger_gesture_by_name("swipe_left"),
            4 => trigger_gesture_by_name("swipe_right"),
            _ => {}
        },
        _ => {}
    }
}

fn parse_button_str(s: &str) -> u8 {
    match s.to_ascii_lowercase().as_str() {
        "right" | "r" => 1,
        "middle" | "m" => 2,
        _ => 0,
    }
}

fn process_subpixel_move(dx: f32, dy: f32, sensitivity: f32, state: &Arc<Mutex<TrackpadState>>) {
    let mut s = state.lock().unwrap();
    let total_x = s.acc_x + (dx * sensitivity);
    let step_x = total_x.trunc() as i32;
    s.acc_x = total_x.fract();

    let total_y = s.acc_y + (dy * sensitivity);
    let step_y = total_y.trunc() as i32;
    s.acc_y = total_y.fract();

    if step_x != 0 || step_y != 0 {
        send_mouse_move(step_x, step_y);
    }
}

fn process_scroll(dx: f32, dy: f32, state: &Arc<Mutex<TrackpadState>>) {
    let now = Instant::now();
    let mut s = state.lock().unwrap();

    let dt = s
        .last_scroll_time
        .map(|t| (now - t).as_secs_f32())
        .unwrap_or(0.016)
        .max(0.001);
    s.last_scroll_time = Some(now);

    s.scroll_vel_y = (dy / dt).clamp(-15.0, 15.0);
    s.scroll_vel_x = (dx / dt).clamp(-15.0, 15.0);

    let total_y = s.scroll_acc_y + dy;
    let step_y = total_y.trunc() as i32;
    s.scroll_acc_y = total_y.fract();

    let total_x = s.scroll_acc_x + dx;
    let step_x = total_x.trunc() as i32;
    s.scroll_acc_x = total_x.fract();

    if step_y != 0 || step_x != 0 {
        send_wheel_scroll(step_y, step_x);
    }
}
