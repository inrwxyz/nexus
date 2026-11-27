//! Unicode voice dictation at the active cursor via Win32 SendInput (`KEYEVENTF_UNICODE`).
//! Supports both ultra-fast raw dictation ("type ...") and optional AI-formatted typing ("smart type ...").

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationIntent {
    Raw(String),
    Smart(String),
}

/// Detects explicit voice typing triggers without colliding with command execution.
pub fn parse_dictation_intent(cmd: &str) -> Option<DictationIntent> {
    let trimmed = cmd.trim();
    let lower = trimmed.to_ascii_lowercase();

    for prefix in [
        "smart type ",
        "smart dictate ",
        "clean type ",
        "polish and type ",
        "format and type ",
    ] {
        if lower.starts_with(prefix) {
            let body = trimmed[prefix.len()..].trim();
            if !body.is_empty() {
                return Some(DictationIntent::Smart(body.to_string()));
            }
        }
    }

    for prefix in ["type this ", "dictate ", "type ", "write "] {
        if lower.starts_with(prefix) {
            let body = trimmed[prefix.len()..].trim();
            if !body.is_empty() {
                return Some(DictationIntent::Raw(body.to_string()));
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
pub fn type_unicode_at_cursor(text: &str) -> Result<usize, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VK_RETURN,
    };

    if text.is_empty() {
        return Ok(0);
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            for flags in [0u32, KEYEVENTF_KEYUP] {
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_RETURN,
                            wScan: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
            continue;
        }
        let mut buf = [0u16; 2];
        for &mut code_unit in ch.encode_utf16(&mut buf) {
            for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: 0,
                            wScan: code_unit,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                });
            }
        }
    }

    if inputs.is_empty() {
        return Ok(0);
    }

    // Small settle delay ensures any voice-pill activation doesn't steal focus before typing
    std::thread::sleep(Duration::from_millis(25));
    let mut total_sent = 0usize;
    for chunk in inputs.chunks(64) {
        let sent = unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        total_sent += sent as usize;
        if chunk.len() == 64 {
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    Ok(total_sent)
}

#[cfg(not(target_os = "windows"))]
pub fn type_unicode_at_cursor(text: &str) -> Result<usize, String> {
    Ok(text.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_and_smart_dictation_cleanly() {
        assert_eq!(
            parse_dictation_intent("type hello how are you"),
            Some(DictationIntent::Raw("hello how are you".to_string()))
        );
        assert_eq!(
            parse_dictation_intent("smart type kal office asbo na sorry"),
            Some(DictationIntent::Smart("kal office asbo na sorry".to_string()))
        );
        assert_eq!(parse_dictation_intent("open chrome"), None);
    }
}
