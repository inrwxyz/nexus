use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT,
};

extern "system" {
    pub fn MessageBeep(u_type: u32) -> i32;
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

pub fn scroll(amount: i32) {
    unsafe {
        let input = INPUT {
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

        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}
