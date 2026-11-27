//! Win32 mouse, wheel, keyboard, Unicode text, gesture, and caret focus helpers for Trackpad.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL,
};

pub(crate) static LEFT_HELD: AtomicBool = AtomicBool::new(false);

const KEYEVENTF_KEYUP_FLAG: u32 = 0x0002;

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
struct CURSORINFO {
    cb_size: u32,
    flags: u32,
    h_cursor: usize,
    pt_screen_pos: [i32; 2],
}

#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
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
            if GetGUIThreadInfo(tid, &mut gui) != 0 && (gui.hwnd_caret != 0 || (gui.flags & 1) != 0) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn check_and_notify_input_focus(socket: &UdpSocket, dst: std::net::SocketAddr) {
    let Ok(s) = socket.try_clone() else { return };
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        let msg: &[u8] = if is_text_input_focused() {
            b"{\"type\":\"keyboard_auto\",\"open\":true}"
        } else {
            b"{\"type\":\"keyboard_auto\",\"open\":false}"
        };
        let _ = s.send_to(msg, dst);
    });
}

pub(crate) fn send_mouse_move(dx: i32, dy: i32) {
    unsafe { mouse_event(MOUSEEVENTF_MOVE, dx as u32, dy as u32, 0, 0); }
}

pub(crate) fn send_mouse_down(btn: u8) {
    if btn == 0 {
        LEFT_HELD.store(true, Ordering::Relaxed);
    }
    let flag = match btn {
        1 => MOUSEEVENTF_RIGHTDOWN,
        2 => MOUSEEVENTF_MIDDLEDOWN,
        _ => MOUSEEVENTF_LEFTDOWN,
    };
    unsafe { mouse_event(flag, 0, 0, 0, 0); }
}

pub(crate) fn send_mouse_up(btn: u8) {
    if btn == 0 {
        LEFT_HELD.store(false, Ordering::Relaxed);
    }
    let flag = match btn {
        1 => MOUSEEVENTF_RIGHTUP,
        2 => MOUSEEVENTF_MIDDLEUP,
        _ => MOUSEEVENTF_LEFTUP,
    };
    unsafe { mouse_event(flag, 0, 0, 0, 0); }
}

pub(crate) fn send_mouse_click(btn: u8) {
    send_mouse_down(btn);
    thread::sleep(Duration::from_millis(15));
    send_mouse_up(btn);
}

pub(crate) fn send_wheel_scroll(dy: i32, dx: i32) {
    unsafe {
        if dy != 0 {
            mouse_event(MOUSEEVENTF_WHEEL, 0, 0, (dy * 6) as u32, 0);
        }
        if dx != 0 {
            mouse_event(MOUSEEVENTF_HWHEEL, 0, 0, (dx * 6) as u32, 0);
        }
    }
}

pub(crate) fn handle_key(k: &str) {
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
    const EXT: u32 = 0x0001;
    let ext = if matches!(vk, 0x25..=0x28 | 0x5B | 0x5C) { EXT } else { 0 };
    unsafe {
        keybd_event(vk, 0, ext, 0);
        thread::sleep(Duration::from_millis(15));
        keybd_event(vk, 0, ext | KEYEVENTF_KEYUP_FLAG, 0);
    }
}

fn send_unicode_units(units: &[u16]) {
    let mut inputs = Vec::with_capacity(units.len() * 2);
    for &u in units {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: 0, wScan: u, dwFlags: KEYEVENTF_UNICODE, time: 0, dwExtraInfo: 0 },
            },
        });
    }
    for &u in units {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: u,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

pub(crate) fn handle_text(t: &str) {
    let mut utf16_buf = [0u16; 2];
    for ch in t.chars() {
        match ch {
            '\n' | '\r' => { send_single_key(0x0D); continue; }
            '\t' => { send_single_key(0x09); continue; }
            '\x08' => { send_single_key(0x08); continue; }
            _ => {}
        }

        if ch.is_ascii() {
            let scan = unsafe { VkKeyScanW(ch as u16) };
            let mods = (scan >> 8) & 0xFF;
            if scan != -1 && (mods & 6) == 0 {
                let vk = (scan & 0xFF) as u8;
                let shift = (mods & 1) != 0;
                unsafe {
                    if shift { keybd_event(0x10, 0, 0, 0); }
                    keybd_event(vk, 0, 0, 0);
                    thread::sleep(Duration::from_millis(6));
                    keybd_event(vk, 0, KEYEVENTF_KEYUP_FLAG, 0);
                    if shift { keybd_event(0x10, 0, KEYEVENTF_KEYUP_FLAG, 0); }
                }
                continue;
            }
        }

        let units = ch.encode_utf16(&mut utf16_buf);
        send_unicode_units(units);
    }
}

pub(crate) fn trigger_gesture_by_name(name: &str) {
    match name {
        "swipe_up" => send_key_combo(&[0x5B, 0x09]),
        "swipe_down" => send_key_combo(&[0x5B, 0x44]),
        "swipe_left" => send_key_combo(&[0x11, 0x5B, 0x25]),
        "swipe_right" => send_key_combo(&[0x11, 0x5B, 0x27]),
        _ => {}
    }
}

pub fn press_combo(vks: &[u8]) {
    send_key_combo(vks);
}

fn send_key_combo(vks: &[u8]) {
    const EXT: u32 = 0x0001;
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
