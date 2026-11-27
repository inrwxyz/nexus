//! Hardware ScanCode keyboard & relative mouse input synthesis via Win32 `SendInput`.
//! Uses `KEYEVENTF_SCANCODE` so DirectX, Unreal, Unity, racing, and shooter games accept held keys.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT,
};

const MAPVK_VK_TO_VSC: u32 = 0;

fn is_extended_vk(vk: u8) -> bool {
    matches!(
        vk,
        0x21..=0x28 // PageUp, PageDown, End, Home, Left, Up, Right, Down
            | 0x2D..=0x2E // Insert, Delete
            | 0x5B..=0x5C // Left/Right Win
            | 0x6F // Numpad /
            | 0xA3 // Right Ctrl
            | 0xA5 // Right Alt
    )
}

/// Sends a hardware scancode key down or key up for the given Windows Virtual-Key code.
pub(crate) fn send_vk_scancode(vk: u8, down: bool) {
    if vk == 0 {
        return;
    }
    let sc = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    if sc == 0 {
        return;
    }
    let mut flags = KEYEVENTF_SCANCODE;
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended_vk(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: sc,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

pub(crate) fn send_mouse_delta(dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

pub(crate) fn send_mouse_button(right: bool, down: bool) {
    let flags = match (right, down) {
        (false, true) => MOUSEEVENTF_LEFTDOWN,
        (false, false) => MOUSEEVENTF_LEFTUP,
        (true, true) => MOUSEEVENTF_RIGHTDOWN,
        (true, false) => MOUSEEVENTF_RIGHTUP,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}
