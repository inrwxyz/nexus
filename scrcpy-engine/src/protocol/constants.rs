pub const SCRCPY_VERSION: &str = "3.3.4";
pub const DEVICE_SERVER_PATH: &str = "/data/local/tmp/scrcpy-server.jar";

pub const DEFAULT_BIT_RATE: u32 = 8_000_000; // 8 Mbps
pub const DEFAULT_MAX_SIZE: u16 = 1920;
pub const DEFAULT_MAX_FPS: u8 = 60;

// Scrcpy v3 Control Message Types
pub const CONTROL_MSG_TYPE_INJECT_KEYCODE: u8 = 0;
pub const CONTROL_MSG_TYPE_INJECT_TEXT: u8 = 1;
pub const CONTROL_MSG_TYPE_INJECT_TOUCH_EVENT: u8 = 2;
pub const CONTROL_MSG_TYPE_INJECT_SCROLL_EVENT: u8 = 3;
pub const CONTROL_MSG_TYPE_BACK_OR_SCREEN_ON: u8 = 4;
pub const CONTROL_MSG_TYPE_EXPAND_NOTIFICATION_PANEL: u8 = 5;
pub const CONTROL_MSG_TYPE_EXPAND_SETTINGS_PANEL: u8 = 6;
pub const CONTROL_MSG_TYPE_COLLAPSE_PANELS: u8 = 7;
pub const CONTROL_MSG_TYPE_GET_CLIPBOARD: u8 = 8;
pub const CONTROL_MSG_TYPE_SET_CLIPBOARD: u8 = 9;
pub const CONTROL_MSG_TYPE_SET_DISPLAY_POWER: u8 = 10;
pub const CONTROL_MSG_TYPE_ROTATE_DEVICE: u8 = 11;

// Android MotionEvent actions
pub const AMOTION_EVENT_ACTION_DOWN: u8 = 0;
pub const AMOTION_EVENT_ACTION_UP: u8 = 1;
pub const AMOTION_EVENT_ACTION_MOVE: u8 = 2;
pub const AMOTION_EVENT_ACTION_CANCEL: u8 = 3;

// Android KeyEvent actions
pub const AKEY_EVENT_ACTION_DOWN: u8 = 0;
pub const AKEY_EVENT_ACTION_UP: u8 = 1;

// Special pointer IDs
pub const POINTER_ID_MOUSE: u64 = u64::MAX;
pub const POINTER_ID_GENERIC_FINGER: u64 = u64::MAX - 1;
