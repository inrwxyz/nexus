use std::net::UdpSocket;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL,
};

use crate::settings::AuraSettings;

pub const UDP_PORT: u16 = 41781;

static RUNNING: AtomicBool = AtomicBool::new(true);
use std::sync::Condvar;
static LEFT_HELD: AtomicBool = AtomicBool::new(false);
static INERTIA_WAKE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
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
                s
            }
            Err(e) => {
                eprintln!("[Nexus Trackpad] Failed to bind UDP {}: {}", UDP_PORT, e);
                return;
            }
        };

        let state = Arc::new(Mutex::new(TrackpadState::default()));
        let mut buffer = [0u8; 1024];

        // Momentum inertia scroll loop
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
                thread::sleep(Duration::from_millis(16)); // ~60 FPS decay step
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
                Ok(res) => { last_pkt = Instant::now(); res }
                Err(_) => {
                    // phone disappeared while dragging (lost "up" packet): never leave the button stuck
                    if LEFT_HELD.load(Ordering::Relaxed) && last_pkt.elapsed() > Duration::from_secs(10) {
                        send_mouse_up(0);
                    }
                    continue;
                }
            };

            if len == 0 {
                continue;
            }

            // Check if companion touchpad is enabled
            let (enabled, sensitivity, token) = {
                let s = settings_state.lock().unwrap();
                let master = s.module_enabled("companion");
                let tp = s.companion.touchpad;
                (master && tp, s.companion.sensitivity, s.companion.auth_token.clone())
            };

            if !enabled {
                continue;
            }

            // Dual packet support: JSON or Binary
            if buffer[0] == b'{' {
                if let Ok(pkt) = serde_json::from_slice::<JsonPacket>(&buffer[..len]) {
                    // Token is mandatory. Unpaired / wrong token => ignore and tell the phone once in a while.
                    match &token {
                        Some(expected) if &pkt.token == expected => {}
                        _ => {
                            notify_unauthorized(&socket, src, &mut last_unauth);
                            continue;
                        }
                    }

                    handle_json_event(&pkt, sensitivity, &state, &socket, src);
                }
            } else if len >= 16 {
                // Binary packet layout (16-20 bytes)
                let token_hash = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                match &token {
                    Some(expected) if token_hash == crc32fast::hash(expected.as_bytes()) => {}
                    _ => {
                        notify_unauthorized(&socket, src, &mut last_unauth);
                        continue;
                    }
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

/// One tiny reply (max every 2 s) so a phone that was unpaired on the PC notices immediately.
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
            // Cancel active momentum on user touch move
            {
                let mut s = state.lock().unwrap();
                s.inertia_running = false;
            }
            process_subpixel_move(pkt.dx, pkt.dy, sensitivity, state);
        }
        "down" => {
            let btn = parse_button_str(pkt.btn.as_deref().unwrap_or("left"));
            send_mouse_down(btn);
        }
        "up" => {
            let btn = parse_button_str(pkt.btn.as_deref().unwrap_or("left"));
            send_mouse_up(btn);
        }
        "click" => {
            let btn = parse_button_str(pkt.btn.as_deref().unwrap_or("left"));
            send_mouse_click(btn);
            if btn == 0 {
                check_and_notify_input_focus(socket, src);
            }
        }
        "scroll" => {
            process_scroll(pkt.dx, pkt.dy, state);
        }
        "scroll_end" => {
            // Trigger momentum if velocities were non-zero
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
            // Move
            {
                let mut s = state.lock().unwrap();
                s.inertia_running = false;
            }
            process_subpixel_move(dx, dy, sensitivity, state);
        }
        2 => {
            // Mouse Down (param = 0:left, 1:right, 2:middle)
            send_mouse_down(param);
        }
        3 => {
            // Mouse Up
            send_mouse_up(param);
        }
        4 => {
            // Mouse Click
            send_mouse_click(param);
            if param == 0 {
                check_and_notify_input_focus(socket, src);
            }
        }
        5 => {
            // Scroll
            process_scroll(dx, dy, state);
            if (flags & 0x01) != 0 {
                {
                    let mut s = state.lock().unwrap();
                    s.inertia_running = true;
                }
                wake_inertia();
            }
        }
        6 => {
            // Gesture (param: 1=swipe_up, 2=swipe_down, 3=swipe_left, 4=swipe_right)
            match param {
                1 => trigger_gesture_by_name("swipe_up"),
                2 => trigger_gesture_by_name("swipe_down"),
                3 => trigger_gesture_by_name("swipe_left"),
                4 => trigger_gesture_by_name("swipe_right"),
                _ => {}
            }
        }
        _ => {}
    }
}

fn parse_button_str(s: &str) -> u8 {
    match s.to_ascii_lowercase().as_str() {
        "right" | "r" => 1,
        "middle" | "m" => 2,
        _ => 0, // default left
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

    // Track smoothed velocity for inertia
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

#[repr(C)]
struct CURSORINFO {
    cb_size: u32,
    flags: u32,
    h_cursor: usize,
    pt_screen_pos: [i32; 2],
}

#[repr(C)]
struct GUITHREADINFO {
    cb_size: u32,
    flags: u32,
    hwnd_active: usize,
    hwnd_focus: usize,
    hwnd_capture: usize,
    hwnd_menu_owner: usize,
    hwnd_move_size: usize,
    hwnd_caret: usize,
    rc_caret: [i32; 4],
}

#[link(name = "user32")]
extern "system" {
    fn mouse_event(dwFlags: u32, dx: u32, dy: u32, dwData: u32, dwExtraInfo: usize);
    fn keybd_event(bVk: u8, bScan: u8, dwFlags: u32, dwExtraInfo: usize);
    fn VkKeyScanW(ch: u16) -> i16;
    fn GetCursorInfo(pci: *mut CURSORINFO) -> i32;
    fn LoadCursorW(hInstance: usize, lpCursorName: usize) -> usize;
    fn GetForegroundWindow() -> usize;
    fn GetWindowThreadProcessId(hWnd: usize, lpdwProcessId: *mut u32) -> u32;
    fn GetGUIThreadInfo(idThread: u32, pgui: *mut GUITHREADINFO) -> i32;
}

fn is_text_input_focused() -> bool {
    unsafe {
        let mut ci = CURSORINFO {
            cb_size: std::mem::size_of::<CURSORINFO>() as u32,
            flags: 0,
            h_cursor: 0,
            pt_screen_pos: [0, 0],
        };
        if GetCursorInfo(&mut ci) != 0 {
            let ibeam = LoadCursorW(0, 32513); // IDC_IBEAM
            if ci.h_cursor != 0 && ci.h_cursor == ibeam {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg != 0 {
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(fg, &mut pid);
            let mut gui = GUITHREADINFO {
                cb_size: std::mem::size_of::<GUITHREADINFO>() as u32,
                flags: 0,
                hwnd_active: 0,
                hwnd_focus: 0,
                hwnd_capture: 0,
                hwnd_menu_owner: 0,
                hwnd_move_size: 0,
                hwnd_caret: 0,
                rc_caret: [0, 0, 0, 0],
            };
            if GetGUIThreadInfo(tid, &mut gui) != 0 {
                if gui.hwnd_caret != 0 || (gui.flags & 1) != 0 {
                    return true;
                }
            }
        }
    }
    false
}

fn check_and_notify_input_focus(socket: &UdpSocket, dst: std::net::SocketAddr) {
    let s = match socket.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        let is_input = is_text_input_focused();
        let msg: &[u8] = if is_input {
            b"{\"type\":\"keyboard_auto\",\"open\":true}"
        } else {
            b"{\"type\":\"keyboard_auto\",\"open\":false}"
        };
        let _ = s.send_to(msg, dst);
    });
}

fn send_mouse_move(dx: i32, dy: i32) {
    unsafe {
        mouse_event(
            MOUSEEVENTF_MOVE,
            dx as u32,
            dy as u32,
            0,
            0,
        );
    }
}

fn send_mouse_down(btn: u8) {
    if btn == 0 { LEFT_HELD.store(true, Ordering::Relaxed); }
    let flag = match btn {
        1 => MOUSEEVENTF_RIGHTDOWN,
        2 => MOUSEEVENTF_MIDDLEDOWN,
        _ => MOUSEEVENTF_LEFTDOWN,
    };
    unsafe {
        mouse_event(flag, 0, 0, 0, 0);
    }
}

fn send_mouse_up(btn: u8) {
    if btn == 0 { LEFT_HELD.store(false, Ordering::Relaxed); }
    let flag = match btn {
        1 => MOUSEEVENTF_RIGHTUP,
        2 => MOUSEEVENTF_MIDDLEUP,
        _ => MOUSEEVENTF_LEFTUP,
    };
    unsafe {
        mouse_event(flag, 0, 0, 0, 0);
    }
}

fn send_mouse_click(btn: u8) {
    send_mouse_down(btn);
    thread::sleep(Duration::from_millis(15));
    send_mouse_up(btn);
}

fn send_wheel_scroll(dy: i32, dx: i32) {
    unsafe {
        if dy != 0 {
            mouse_event(
                MOUSEEVENTF_WHEEL,
                0,
                0,
                (dy * 6) as u32,
                0,
            );
        }
        if dx != 0 {
            mouse_event(
                MOUSEEVENTF_HWHEEL,
                0,
                0,
                (dx * 6) as u32,
                0,
            );
        }
    }
}

fn handle_key(k: &str) {
    match k.to_ascii_lowercase().as_str() {
        "backspace" | "back" => send_single_key(0x08),
        "enter" | "return" => send_single_key(0x0D),
        "tab" => send_single_key(0x09),
        "escape" | "esc" => send_single_key(0x1B),
        "space" => send_single_key(0x20),
        "up" => send_single_key(0x26),
        "down" => send_single_key(0x28),
        "left" => send_single_key(0x25),
        "right" => send_single_key(0x27),
        "win" => send_single_key(0x5B),
        "volume_up" | "volup" => send_single_key(0xAF),
        "volume_down" | "voldown" => send_single_key(0xAE),
        "volume_mute" | "volmute" => send_single_key(0xAD),
        _ => {}
    }
}

fn send_single_key(vk: u8) {
    const EXT: u32 = 0x0001; // KEYEVENTF_EXTENDEDKEY
    let ext = if matches!(vk, 0x25..=0x28 | 0x5B | 0x5C) { EXT } else { 0 };
    unsafe {
        keybd_event(vk, 0, ext, 0);
        thread::sleep(Duration::from_millis(15));
        keybd_event(vk, 0, ext | KEYEVENTF_KEYUP_FLAG, 0);
    }
}

fn handle_text(t: &str) {
    for c in t.encode_utf16() {
        if c == b'\n' as u16 || c == b'\r' as u16 {
            send_single_key(0x0D);
            continue;
        }
        if c == b'\t' as u16 {
            send_single_key(0x09);
            continue;
        }
        if c == b'\x08' as u16 {
            send_single_key(0x08);
            continue;
        }
        let scan = unsafe { VkKeyScanW(c) };
        if scan != -1 {
            let vk = (scan & 0xFF) as u8;
            let shift = ((scan >> 8) & 1) != 0;
            let ctrl = ((scan >> 8) & 2) != 0;
            let alt = ((scan >> 8) & 4) != 0;

            unsafe {
                if shift { keybd_event(0x10, 0, 0, 0); }
                if ctrl { keybd_event(0x11, 0, 0, 0); }
                if alt { keybd_event(0x12, 0, 0, 0); }

                keybd_event(vk, 0, 0, 0);
                thread::sleep(Duration::from_millis(6));
                keybd_event(vk, 0, KEYEVENTF_KEYUP_FLAG, 0);

                if alt { keybd_event(0x12, 0, KEYEVENTF_KEYUP_FLAG, 0); }
                if ctrl { keybd_event(0x11, 0, KEYEVENTF_KEYUP_FLAG, 0); }
                if shift { keybd_event(0x10, 0, KEYEVENTF_KEYUP_FLAG, 0); }
            }
        }
    }
}

fn trigger_gesture_by_name(name: &str) {
    match name {
        "swipe_up" => {
            // 3-finger swipe UP: Task View (Win + Tab)
            send_key_combo(&[0x5B, 0x09]); // VK_LWIN, VK_TAB
        }
        "swipe_down" => {
            // 3-finger swipe DOWN: Show Desktop (Win + D)
            send_key_combo(&[0x5B, 0x44]); // VK_LWIN, 'D'
        }
        "swipe_left" => {
            // 3-finger swipe LEFT: Virtual Desktop Left (Ctrl + Win + Left)
            send_key_combo(&[0x11, 0x5B, 0x25]); // VK_CONTROL, VK_LWIN, VK_LEFT
        }
        "swipe_right" => {
            // 3-finger swipe RIGHT: Virtual Desktop Right (Ctrl + Win + Right)
            send_key_combo(&[0x11, 0x5B, 0x27]); // VK_CONTROL, VK_LWIN, VK_RIGHT
        }
        _ => {}
    }
}

const KEYEVENTF_KEYUP_FLAG: u32 = 0x0002;

pub fn press_combo(vks: &[u8]) { send_key_combo(vks) }

fn send_key_combo(vks: &[u8]) {
    const EXT: u32 = 0x0001; // KEYEVENTF_EXTENDEDKEY
    let ext = |vk: u8| if matches!(vk, 0x25..=0x28 | 0x5B | 0x5C) { EXT } else { 0 };
    unsafe {
        for &vk in vks {
            keybd_event(vk, 0, ext(vk), 0);
        }
        thread::sleep(Duration::from_millis(25));
        for &vk in vks.iter().rev() {
            keybd_event(vk, 0, ext(vk) | KEYEVENTF_KEYUP_FLAG, 0);
        }
    }
}
