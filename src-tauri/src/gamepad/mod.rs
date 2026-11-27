//! Zero-latency binary UDP game controller receiver + hardware scancode / mouse state machine.
//! Safety watchdog automatically releases all held keys and mouse buttons after 420 ms of silence.

pub mod profiles;
mod scancode;

use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::settings::{AuraSettings, GamepadConfig};
use scancode::{send_mouse_button, send_mouse_delta, send_vk_scancode};

pub const PKT_GAMEPAD: u8 = 0x10;
const WATCHDOG_TIMEOUT: Duration = Duration::from_millis(420);

static GAMEPAD_ENABLED: AtomicBool = AtomicBool::new(true);
static WATCHDOG_STARTED: OnceLock<()> = OnceLock::new();

// Fixed button bitmask -> Windows Virtual Key mapping
const BTN_GAS: u16 = 1 << 0; // W
const BTN_BRAKE: u16 = 1 << 1; // S
const BTN_LEFT: u16 = 1 << 2; // A
const BTN_RIGHT: u16 = 1 << 3; // D
const BTN_SPRINT: u16 = 1 << 4; // Shift (VK_LSHIFT = 0xA0)
const BTN_JUMP: u16 = 1 << 5; // Space (0x20)
const BTN_FIRE: u16 = 1 << 6; // Mouse Left
const BTN_AIM: u16 = 1 << 7; // Mouse Right
const BTN_RELOAD: u16 = 1 << 8; // R
const BTN_CROUCH: u16 = 1 << 9; // C
const BTN_INTERACT: u16 = 1 << 10; // E
const BTN_ALT: u16 = 1 << 11; // F
const BTN_CAM: u16 = 1 << 12; // V
const BTN_LOOK: u16 = 1 << 13; // Q
const BTN_ESC: u16 = 1 << 14; // Esc
const BTN_EXTRA: u16 = 1 << 15; // G

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedGamepadPacket {
    pub flags: u8,
    pub buttons: u16,
    pub lx: f32,
    pub ly: f32,
    pub aim_dx: i16,
    pub aim_dy: i16,
    pub custom_vks: [u8; 4],
}

#[derive(Default)]
struct HeldState {
    held_vks: BTreeSet<u8>,
    mouse_left: bool,
    mouse_right: bool,
    aim_acc_x: f32,
    aim_acc_y: f32,
    last_packet_at: Option<Instant>,
    total_packets: u64,
    active_mode_flag: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct GamepadStatus {
    pub enabled: bool,
    pub connected: bool,
    pub held_keys_count: usize,
    pub total_packets: u64,
    pub vigem_installed: bool,
}

fn state_lock() -> &'static Mutex<HeldState> {
    static STATE: OnceLock<Mutex<HeldState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HeldState::default()))
}

fn ensure_watchdog() {
    WATCHDOG_STARTED.get_or_init(|| {
        thread::Builder::new()
            .name("NexusGamepadWatchdog".into())
            .spawn(|| loop {
                thread::sleep(Duration::from_millis(100));
                let should_release = {
                    let g = state_lock().lock().unwrap_or_else(|e| e.into_inner());
                    let has_held = !g.held_vks.is_empty() || g.mouse_left || g.mouse_right;
                    has_held
                        && g.last_packet_at
                            .map(|t| t.elapsed() > WATCHDOG_TIMEOUT)
                            .unwrap_or(true)
                };
                if should_release {
                    release_all();
                }
            })
            .ok();
    });
}

pub fn sync_with_settings(settings: &AuraSettings) {
    let want = settings.module_should_run_now("gamepad")
        && settings.module_should_run_now("companion")
        && settings.companion.auth_token.is_some();
    GAMEPAD_ENABLED.store(want, Ordering::SeqCst);
    if !want {
        release_all();
    }
}

pub fn is_enabled() -> bool {
    GAMEPAD_ENABLED.load(Ordering::SeqCst)
}

/// Immediately releases any held keyboard scan codes and mouse buttons.
pub fn release_all() {
    let mut g = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    let vks: Vec<u8> = g.held_vks.iter().copied().collect();
    g.held_vks.clear();
    let ml = g.mouse_left;
    let mr = g.mouse_right;
    g.mouse_left = false;
    g.mouse_right = false;
    g.aim_acc_x = 0.0;
    g.aim_acc_y = 0.0;
    drop(g);

    for vk in vks {
        send_vk_scancode(vk, false);
    }
    if ml {
        send_mouse_button(false, false);
    }
    if mr {
        send_mouse_button(true, false);
    }
}

/// Decode 24-byte (or 20-byte legacy) binary gamepad packet after token hash verification.
pub fn decode_packet(buf: &[u8]) -> Option<DecodedGamepadPacket> {
    if buf.len() < 16 || buf[4] != PKT_GAMEPAD {
        return None;
    }
    let flags = buf[5];
    let buttons = u16::from_le_bytes([buf[6], buf[7]]);
    let lx_raw = i16::from_le_bytes([buf[8], buf[9]]);
    let ly_raw = i16::from_le_bytes([buf[10], buf[11]]);
    let aim_dx = i16::from_le_bytes([buf[12], buf[13]]);
    let aim_dy = i16::from_le_bytes([buf[14], buf[15]]);
    let custom_vks = if buf.len() >= 20 {
        [buf[16], buf[17], buf[18], buf[19]]
    } else {
        [0, 0, 0, 0]
    };

    Some(DecodedGamepadPacket {
        flags,
        buttons,
        lx: (lx_raw as f32 / 32767.0).clamp(-1.0, 1.0),
        ly: (ly_raw as f32 / 32767.0).clamp(-1.0, 1.0),
        aim_dx,
        aim_dy,
        custom_vks,
    })
}

/// Compute desired held Virtual-Key set and mouse buttons from packet + config.
pub fn desired_inputs(pkt: &DecodedGamepadPacket, cfg: &GamepadConfig) -> (BTreeSet<u8>, bool, bool) {
    let mut vks = BTreeSet::new();
    let b = pkt.buttons;

    if (b & BTN_GAS) != 0 { vks.insert(b'W'); }
    if (b & BTN_BRAKE) != 0 { vks.insert(b'S'); }
    if (b & BTN_LEFT) != 0 { vks.insert(b'A'); }
    if (b & BTN_RIGHT) != 0 { vks.insert(b'D'); }
    if (b & BTN_SPRINT) != 0 { vks.insert(0xA0); } // VK_LSHIFT
    if (b & BTN_JUMP) != 0 { vks.insert(0x20); } // VK_SPACE
    if (b & BTN_RELOAD) != 0 { vks.insert(b'R'); }
    if (b & BTN_CROUCH) != 0 { vks.insert(b'C'); }
    if (b & BTN_INTERACT) != 0 { vks.insert(b'E'); }
    if (b & BTN_ALT) != 0 { vks.insert(b'F'); }
    if (b & BTN_CAM) != 0 { vks.insert(b'V'); }
    if (b & BTN_LOOK) != 0 { vks.insert(b'Q'); }
    if (b & BTN_ESC) != 0 { vks.insert(0x1B); } // VK_ESCAPE
    if (b & BTN_EXTRA) != 0 { vks.insert(b'G'); }

    // Analog stick / gyro steering axes -> WASD + optional edge auto-sprint
    let dz = cfg.gyro_deadzone.clamp(0.02, 0.45);
    let gyro_on = (pkt.flags & 0x01) != 0;
    let x_thresh = if gyro_on { dz } else { 0.25 };
    let y_thresh = 0.25;

    if pkt.lx <= -x_thresh {
        vks.insert(b'A');
    } else if pkt.lx >= x_thresh {
        vks.insert(b'D');
    }

    if pkt.ly >= y_thresh {
        vks.insert(b'W');
        if cfg.auto_sprint && !gyro_on && pkt.ly >= 0.88 {
            vks.insert(0xA0);
        }
    } else if pkt.ly <= -y_thresh {
        vks.insert(b'S');
    }

    let mut ml = (b & BTN_FIRE) != 0;
    let mut mr = (b & BTN_AIM) != 0;

    // Arbitrary remapped / custom buttons from the phone HUD editor (`0xF1` = LMB, `0xF2` = RMB)
    for vk in pkt.custom_vks {
        match vk {
            0 => {}
            0xF1 => ml = true,
            0xF2 => mr = true,
            code => {
                vks.insert(code);
            }
        }
    }

    (vks, ml, mr)
}

pub fn handle_binary_packet(buf: &[u8], cfg: &GamepadConfig) {
    if !is_enabled() {
        return;
    }
    ensure_watchdog();
    let Some(pkt) = decode_packet(buf) else {
        return;
    };

    let (want_vks, want_ml, want_mr) = desired_inputs(&pkt, cfg);

    let (to_release, to_press, ml_change, mr_change, move_dx, move_dy) = {
        let mut g = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        g.last_packet_at = Some(Instant::now());
        g.total_packets = g.total_packets.saturating_add(1);
        g.active_mode_flag = pkt.flags;

        let rel: Vec<u8> = g.held_vks.difference(&want_vks).copied().collect();
        let press: Vec<u8> = want_vks.difference(&g.held_vks).copied().collect();
        g.held_vks = want_vks;

        let ml_chg = (g.mouse_left != want_ml).then_some(want_ml);
        let mr_chg = (g.mouse_right != want_mr).then_some(want_mr);
        g.mouse_left = want_ml;
        g.mouse_right = want_mr;

        let aim_sens = cfg.aim_sensitivity.clamp(0.2, 5.0);
        g.aim_acc_x += pkt.aim_dx as f32 * aim_sens;
        g.aim_acc_y += pkt.aim_dy as f32 * aim_sens;
        let dx = g.aim_acc_x.trunc() as i32;
        let dy = g.aim_acc_y.trunc() as i32;
        g.aim_acc_x -= dx as f32;
        g.aim_acc_y -= dy as f32;

        (rel, press, ml_chg, mr_chg, dx, dy)
    };

    for vk in to_release {
        send_vk_scancode(vk, false);
    }
    for vk in to_press {
        send_vk_scancode(vk, true);
    }
    if let Some(down) = ml_change {
        send_mouse_button(false, down);
    }
    if let Some(down) = mr_change {
        send_mouse_button(true, down);
    }
    if move_dx != 0 || move_dy != 0 {
        send_mouse_delta(move_dx, move_dy);
    }
}

/// Checks if ViGEmBus virtual controller driver is registered on Windows (read-only registry query).
pub fn is_vigem_installed() -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY_LOCAL_MACHINE, KEY_READ};
        let key: Vec<u16> = "SYSTEM\\CurrentControlSet\\Services\\ViGEmBus\0".encode_utf16().collect();
        unsafe {
            let mut hkey = std::mem::zeroed();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, key.as_ptr(), 0, KEY_READ, &mut hkey) == 0 {
                RegCloseKey(hkey);
                return true;
            }
        }
    }
    false
}

#[tauri::command]
pub fn get_gamepad_status() -> GamepadStatus {
    let g = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    let connected = g
        .last_packet_at
        .map(|t| t.elapsed() < Duration::from_secs(3))
        .unwrap_or(false);
    GamepadStatus {
        enabled: is_enabled(),
        connected,
        held_keys_count: g.held_vks.len() + usize::from(g.mouse_left) + usize::from(g.mouse_right),
        total_packets: g.total_packets,
        vigem_installed: is_vigem_installed(),
    }
}

#[tauri::command]
pub fn release_gamepad_inputs() -> bool {
    release_all();
    true
}

#[tauri::command]
pub fn get_gamepad_profiles() -> profiles::GamepadProfilesStore {
    profiles::get_store_snapshot()
}

#[tauri::command]
pub fn select_gamepad_profile(profile_id: String, send_to_phone: bool) -> Result<profiles::GamepadProfilesStore, String> {
    if !is_enabled() {
        return Err("Game Controller is suspended (enable Phone Link & pair a phone)".into());
    }
    profiles::select_profile(&profile_id, send_to_phone)
}

#[tauri::command]
pub fn save_gamepad_profile(profile: profiles::GamepadProfile, make_active: bool) -> profiles::GamepadProfilesStore {
    profiles::upsert_profile(profile, make_active, make_active && is_enabled())
}

#[tauri::command]
pub fn delete_gamepad_profile(profile_id: String) -> Result<profiles::GamepadProfilesStore, String> {
    profiles::delete_profile(&profile_id)
}

#[tauri::command]
pub fn reset_gamepad_preset(profile_id: String) -> Result<profiles::GamepadProfilesStore, String> {
    profiles::reset_preset(&profile_id, is_enabled())
}

#[tauri::command]
pub fn push_active_gamepad_profile() -> bool {
    if !is_enabled() {
        return false;
    }
    let st = profiles::get_store_snapshot();
    profiles::notify_phone_profile(&st.active_profile_id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_24_byte_gamepad_packet_and_maps_keys_and_custom_vks() {
        let mut pkt = [0u8; 24];
        pkt[4] = PKT_GAMEPAD;
        pkt[5] = 0x01; // gyro active
        pkt[6..8].copy_from_slice(&(BTN_GAS | BTN_FIRE).to_le_bytes());
        pkt[8..10].copy_from_slice(&(-16384i16).to_le_bytes()); // tilt left ~ -0.5
        pkt[10..12].copy_from_slice(&(31000i16).to_le_bytes());
        pkt[12..14].copy_from_slice(&(12i16).to_le_bytes());
        pkt[14..16].copy_from_slice(&(-4i16).to_le_bytes());
        pkt[16] = b'Z';
        pkt[17] = 0xF2; // custom Right Mouse

        let dec = decode_packet(&pkt).expect("valid packet");
        assert_eq!(dec.flags, 0x01);
        assert_eq!(dec.aim_dx, 12);
        assert_eq!(dec.aim_dy, -4);
        assert!(dec.lx < -0.45);

        let cfg = GamepadConfig::default();
        let (vks, ml, mr) = desired_inputs(&dec, &cfg);
        assert!(vks.contains(&b'W'));
        assert!(vks.contains(&b'A'));
        assert!(vks.contains(&b'Z'));
        assert!(ml);
        assert!(mr);
    }

    #[test]
    fn deadzone_prevents_jitter_and_full_stick_enables_auto_sprint() {
        let mut pkt = [0u8; 24];
        pkt[4] = PKT_GAMEPAD;
        pkt[5] = 0x00; // touch stick mode
        pkt[8..10].copy_from_slice(&(2000i16).to_le_bytes()); // ~0.06 below threshold
        pkt[10..12].copy_from_slice(&(31500i16).to_le_bytes()); // ~0.96 -> W + Shift

        let dec = decode_packet(&pkt).unwrap();
        let cfg = GamepadConfig::default();
        let (vks, ml, mr) = desired_inputs(&dec, &cfg);
        assert!(vks.contains(&b'W'));
        assert!(vks.contains(&0xA0)); // auto-sprint Shift
        assert!(!vks.contains(&b'A'));
        assert!(!vks.contains(&b'D'));
        assert!(!ml && !mr);
    }
}
