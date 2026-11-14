use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT,
};

#[repr(C)]
#[allow(non_snake_case)]
struct LASTINPUTINFO {
    cbSize: u32,
    dwTime: u32,
}

extern "system" {
    pub fn MessageBeep(u_type: u32) -> i32;
    fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;
    fn GetTickCount() -> u32;
}

pub fn set_clipboard(text: &str) -> bool {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = utf16.len() * 2;

    unsafe {
        if OpenClipboard(0 as HWND) != 0 {
            EmptyClipboard();
            let h_mem = GlobalAlloc(0x0002, bytes); // GMEM_MOVEABLE = 0x0002
            if !h_mem.is_null() {
                let p_mem = GlobalLock(h_mem) as *mut u16;
                if !p_mem.is_null() {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), p_mem, utf16.len());
                    GlobalUnlock(h_mem);
                    SetClipboardData(13, h_mem); // 13 = CF_UNICODETEXT
                }
            }
            CloseClipboard();
            return true;
        }
    }
    false
}

pub fn send_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(2, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
pub fn send_shift_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x10,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x10,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
pub fn send_ctrl_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x11,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x11,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

#[allow(dead_code)]
pub fn send_alt_key(vk: u8) {
    unsafe {
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x12, // VK_MENU (Alt)

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: 0,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk as u16,

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,

                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0x12, // VK_MENU (Alt)

                        wScan: 0,

                        dwFlags: KEYEVENTF_KEYUP,

                        time: 0,

                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn scroll(amount: i32) {
    unsafe {
        let mut input = INPUT {
            r#type: INPUT_MOUSE,

            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,

                    dy: 0,

                    mouseData: amount as u32,

                    dwFlags: MOUSEEVENTF_WHEEL,

                    time: 0,

                    dwExtraInfo: 0,
                },
            },
        };

        SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn is_user_busy() -> bool {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };

        if GetLastInputInfo(&mut lii) != 0 {
            let now = GetTickCount();
            let idle_ms = now.saturating_sub(lii.dwTime);
            idle_ms < 7500
        } else {
            false
        }
    }
}
