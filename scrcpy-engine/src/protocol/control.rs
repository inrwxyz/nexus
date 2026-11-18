use byteorder::{BigEndian, ByteOrder};
use crate::protocol::constants::*;

#[derive(Debug, Clone)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    pub screen_width: u16,
    pub screen_height: u16,
}

#[derive(Debug, Clone)]
pub enum ControlMessage {
    InjectKeycode {
        action: u8,
        keycode: u32,
        repeat: u32,
        metastate: u32,
    },
    InjectText {
        text: String,
    },
    InjectTouchEvent {
        action: u8,
        pointer_id: u64,
        position: Position,
        pressure: f32,
        action_button: u32,
        buttons: u32,
    },
    InjectScrollEvent {
        position: Position,
        hscroll: f32, // -16.0 .. 16.0
        vscroll: f32, // -16.0 .. 16.0
        buttons: u32,
    },
    BackOrScreenOn {
        action: u8, // AKEY_EVENT_ACTION_DOWN or UP
    },
    ExpandNotificationPanel,
    ExpandSettingsPanel,
    CollapsePanels,
    SetDisplayPower {
        on: bool,
    },
    RotateDevice,
}

impl ControlMessage {
    /// Serializes the control message into a binary buffer matching scrcpy v3 protocol specs.
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            ControlMessage::InjectKeycode { action, keycode, repeat, metastate } => {
                let mut buf = vec![0u8; 14];
                buf[0] = CONTROL_MSG_TYPE_INJECT_KEYCODE;
                buf[1] = *action;
                BigEndian::write_u32(&mut buf[2..6], *keycode);
                BigEndian::write_u32(&mut buf[6..10], *repeat);
                BigEndian::write_u32(&mut buf[10..14], *metastate);
                buf
            }
            ControlMessage::InjectText { text } => {
                let bytes = text.as_bytes();
                let len = bytes.len().min(300);
                let mut buf = vec![0u8; 1 + 4 + len];
                buf[0] = CONTROL_MSG_TYPE_INJECT_TEXT;
                BigEndian::write_u32(&mut buf[1..5], len as u32);
                buf[5..5 + len].copy_from_slice(&bytes[..len]);
                buf
            }
            ControlMessage::InjectTouchEvent { action, pointer_id, position, pressure, action_button, buttons } => {
                let mut buf = vec![0u8; 32];
                buf[0] = CONTROL_MSG_TYPE_INJECT_TOUCH_EVENT;
                buf[1] = *action;
                BigEndian::write_u64(&mut buf[2..10], *pointer_id);
                // Position (12 bytes)
                BigEndian::write_i32(&mut buf[10..14], position.x);
                BigEndian::write_i32(&mut buf[14..18], position.y);
                BigEndian::write_u16(&mut buf[18..20], position.screen_width);
                BigEndian::write_u16(&mut buf[20..22], position.screen_height);
                // Pressure (u16 fixed point)
                let p_u16 = float_to_u16fp(*pressure);
                BigEndian::write_u16(&mut buf[22..24], p_u16);
                BigEndian::write_u32(&mut buf[24..28], *action_button);
                BigEndian::write_u32(&mut buf[28..32], *buttons);
                buf
            }
            ControlMessage::InjectScrollEvent { position, hscroll, vscroll, buttons } => {
                let mut buf = vec![0u8; 21];
                buf[0] = CONTROL_MSG_TYPE_INJECT_SCROLL_EVENT;
                BigEndian::write_i32(&mut buf[1..5], position.x);
                BigEndian::write_i32(&mut buf[5..9], position.y);
                BigEndian::write_u16(&mut buf[9..11], position.screen_width);
                BigEndian::write_u16(&mut buf[11..13], position.screen_height);
                // Norm scroll [-16, 16] -> [-1, 1] -> i16 fixed point
                let h_norm = (*hscroll / 16.0).clamp(-1.0, 1.0);
                let v_norm = (*vscroll / 16.0).clamp(-1.0, 1.0);
                BigEndian::write_i16(&mut buf[13..15], float_to_i16fp(h_norm));
                BigEndian::write_i16(&mut buf[15..17], float_to_i16fp(v_norm));
                BigEndian::write_u32(&mut buf[17..21], *buttons);
                buf
            }
            ControlMessage::BackOrScreenOn { action } => {
                vec![CONTROL_MSG_TYPE_BACK_OR_SCREEN_ON, *action]
            }
            ControlMessage::ExpandNotificationPanel => {
                vec![CONTROL_MSG_TYPE_EXPAND_NOTIFICATION_PANEL]
            }
            ControlMessage::ExpandSettingsPanel => {
                vec![CONTROL_MSG_TYPE_EXPAND_SETTINGS_PANEL]
            }
            ControlMessage::CollapsePanels => {
                vec![CONTROL_MSG_TYPE_COLLAPSE_PANELS]
            }
            ControlMessage::SetDisplayPower { on } => {
                vec![CONTROL_MSG_TYPE_SET_DISPLAY_POWER, if *on { 1 } else { 0 }]
            }
            ControlMessage::RotateDevice => {
                vec![CONTROL_MSG_TYPE_ROTATE_DEVICE]
            }
        }
    }
}

pub fn float_to_u16fp(f: f32) -> u16 {
    let f = f.clamp(0.0, 1.0);
    let u = (f * 65536.0) as u32;
    if u >= 0xffff {
        0xffff
    } else {
        u as u16
    }
}

pub fn float_to_i16fp(f: f32) -> i16 {
    let f = f.clamp(-1.0, 1.0);
    let i = (f * 32768.0) as i32;
    if i >= 0x7fff {
        0x7fff
    } else if i <= -0x8000 {
        -0x8000
    } else {
        i as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_inject_keycode() {
        let msg = ControlMessage::InjectKeycode {
            action: 1, // AKEY_EVENT_ACTION_UP
            keycode: 0x42, // AKEYCODE_ENTER
            repeat: 5,
            metastate: 0x41,
        };
        let buf = msg.serialize();
        assert_eq!(buf.len(), 14);
        let expected = vec![
            CONTROL_MSG_TYPE_INJECT_KEYCODE,
            0x01,
            0x00, 0x00, 0x00, 0x42,
            0x00, 0x00, 0x00, 0x05,
            0x00, 0x00, 0x00, 0x41,
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_serialize_inject_text() {
        let msg = ControlMessage::InjectText {
            text: "hello, world!".to_string(),
        };
        let buf = msg.serialize();
        assert_eq!(buf.len(), 18);
        assert_eq!(buf[0], CONTROL_MSG_TYPE_INJECT_TEXT);
        assert_eq!(&buf[1..5], &[0x00, 0x00, 0x00, 0x0d]);
        assert_eq!(&buf[5..], b"hello, world!");
    }

    #[test]
    fn test_serialize_inject_touch_event() {
        let msg = ControlMessage::InjectTouchEvent {
            action: 0, // AMOTION_EVENT_ACTION_DOWN
            pointer_id: 0x1234567887654321,
            position: Position {
                x: 100,
                y: 200,
                screen_width: 1080,
                screen_height: 1920,
            },
            pressure: 1.0,
            action_button: 1,
            buttons: 1,
        };
        let buf = msg.serialize();
        assert_eq!(buf.len(), 32);
        let expected = vec![
            CONTROL_MSG_TYPE_INJECT_TOUCH_EVENT,
            0x00,
            0x12, 0x34, 0x56, 0x78, 0x87, 0x65, 0x43, 0x21,
            0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0xc8,
            0x04, 0x38, 0x07, 0x80,
            0xff, 0xff,
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x01,
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_serialize_inject_scroll_event() {
        let msg = ControlMessage::InjectScrollEvent {
            position: Position {
                x: 260,
                y: 1026,
                screen_width: 1080,
                screen_height: 1920,
            },
            hscroll: 16.0,
            vscroll: -16.0,
            buttons: 1,
        };
        let buf = msg.serialize();
        assert_eq!(buf.len(), 21);
        let expected = vec![
            CONTROL_MSG_TYPE_INJECT_SCROLL_EVENT,
            0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x04, 0x02,
            0x04, 0x38, 0x07, 0x80,
            0x7F, 0xFF,
            0x80, 0x00,
            0x00, 0x00, 0x00, 0x01,
        ];
        assert_eq!(buf, expected);
    }
}
