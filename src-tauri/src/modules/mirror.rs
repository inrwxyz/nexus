//! Pure-Rust Native Win32 + Direct3D 11 Screen Mirror Window (`nexus.exe`).
//! Uses Windows Media Foundation (`mirror_decoder.rs`) + `ID3D11VideoProcessor`
//! swapchain rendering + `cpal` WASAPI audio (`mirror_audio.rs`), providing
//! low-latency native windowing, auto-rotation, `F11` fullscreen, and input
//! mapping directly in Rust with zero external binaries or WebView overhead.

use crate::modules::mirror_audio::{start_pcm_player, start_raw_player, NativeAudioPlayer};
use crate::modules::mirror_decoder::{
    AudioCodecKind, AudioMfDecoder, DecodedVideoFrame, H264MfDecoder, MfSession,
};
use nexus_mirror_engine::engine::ActiveSession;
use nexus_mirror_engine::protocol::constants::*;
use nexus_mirror_engine::protocol::control::{ControlMessage, Position};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource,
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory2, IDXGISwapChain1, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT,
    DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT as SYS_POINT, RECT as SYS_RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HBRUSH, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_F11, VK_F12, VK_HOME, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN,
    VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::Shell::ExtractIconExW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsZoomed,
    LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageW, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, GWL_STYLE, HWND_TOP, ICON_BIG, ICON_SMALL, IDC_ARROW,
    MSG, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_SHOW, WHEEL_DELTA,
    WINDOWPLACEMENT, WM_APP, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETICON, WM_SIZE, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

const WM_APP_NEW_FRAME: u32 = WM_APP + 1;
const WM_APP_STOP: u32 = WM_APP + 2;

const AMOTION_EVENT_BUTTON_PRIMARY: u32 = 1 << 0;
const AMOTION_EVENT_BUTTON_SECONDARY: u32 = 1 << 1;
const AMOTION_EVENT_BUTTON_TERTIARY: u32 = 1 << 2;

const AMETA_ALT_ON: u32 = 0x02;
const AMETA_SHIFT_ON: u32 = 0x01;
const AMETA_CTRL_ON: u32 = 0x1000;

const AKEYCODE_HOME: u32 = 3;
const AKEYCODE_BACK: u32 = 4;
const AKEYCODE_0: u32 = 7;
const AKEYCODE_A: u32 = 29;
const AKEYCODE_DPAD_UP: u32 = 19;
const AKEYCODE_DPAD_DOWN: u32 = 20;
const AKEYCODE_DPAD_LEFT: u32 = 21;
const AKEYCODE_DPAD_RIGHT: u32 = 22;
const AKEYCODE_VOLUME_UP: u32 = 24;
const AKEYCODE_VOLUME_DOWN: u32 = 25;
const AKEYCODE_POWER: u32 = 26;
const AKEYCODE_TAB: u32 = 61;
const AKEYCODE_SPACE: u32 = 62;
const AKEYCODE_ENTER: u32 = 66;
const AKEYCODE_DEL: u32 = 67;
const AKEYCODE_GRAVE: u32 = 68;
const AKEYCODE_MINUS: u32 = 69;
const AKEYCODE_EQUALS: u32 = 70;
const AKEYCODE_LEFT_BRACKET: u32 = 71;
const AKEYCODE_RIGHT_BRACKET: u32 = 72;
const AKEYCODE_BACKSLASH: u32 = 73;
const AKEYCODE_SEMICOLON: u32 = 74;
const AKEYCODE_APOSTROPHE: u32 = 75;
const AKEYCODE_SLASH: u32 = 76;
const AKEYCODE_PAGE_UP: u32 = 92;
const AKEYCODE_PAGE_DOWN: u32 = 93;
const AKEYCODE_ESCAPE: u32 = 111;
const AKEYCODE_FORWARD_DEL: u32 = 112;
const AKEYCODE_MOVE_HOME: u32 = 122;
const AKEYCODE_MOVE_END: u32 = 123;
const AKEYCODE_F1: u32 = 131;
const AKEYCODE_APP_SWITCH: u32 = 187;

pub struct MirrorHandle {
    stop_flag: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    audio_muted: Arc<AtomicBool>,
    screen_off: Arc<AtomicBool>,
    hwnd: Arc<AtomicIsize>,
    ctrl_tx: mpsc::Sender<ControlMessage>,
    session: Mutex<Option<ActiveSession>>,
    audio_player: Mutex<Option<NativeAudioPlayer>>,
    window_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    tasks: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
}

impl MirrorHandle {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn set_screen_power(&self, on: bool) {
        self.screen_off.store(!on, Ordering::Relaxed);
        let _ = self.ctrl_tx.try_send(ControlMessage::SetDisplayPower { on });
    }

    pub fn set_audio_muted(&self, muted: bool) {
        self.audio_muted.store(muted, Ordering::Relaxed);
        if let Ok(guard) = self.audio_player.lock() {
            if let Some(player) = guard.as_ref() {
                player.sink().set_muted(muted);
            }
        }
    }

    pub fn stop(&self) {
        if self.stop_flag.swap(true, Ordering::SeqCst) {
            return;
        }
        self.running.store(false, Ordering::SeqCst);

        if self.screen_off.swap(false, Ordering::SeqCst) {
            let _ = self
                .ctrl_tx
                .try_send(ControlMessage::SetDisplayPower { on: true });
        }

        let hwnd = self.hwnd.swap(0, Ordering::SeqCst);
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as _, WM_APP_STOP, 0, 0) };
        }

        if let Ok(mut guard) = self.tasks.lock() {
            for t in guard.drain(..) {
                t.abort();
            }
        }
        if let Ok(mut audio_guard) = self.audio_player.lock() {
            audio_guard.take();
        }
        if let Ok(mut sess_guard) = self.session.lock() {
            if let Some(mut sess) = sess_guard.take() {
                tauri::async_runtime::spawn(async move {
                    sess.shutdown().await;
                });
            }
        }
        if let Ok(mut wt) = self.window_thread.lock() {
            if let Some(h) = wt.take() {
                if h.thread().id() != std::thread::current().id() {
                    let _ = h.join();
                }
            }
        }
    }
}

pub async fn launch_mirror(
    app: AppHandle,
    mut session: ActiveSession,
    turn_screen_off: bool,
    audio_buffer_ms: u16,
    max_fps: u16,
) -> Result<Arc<MirrorHandle>, String> {
    let init_w = session.session_info.width.max(360);
    let init_h = session.session_info.height.max(640);
    let audio_codec = session.audio_codec.clone().unwrap_or_else(|| "raw".into());
    let device_name = session.device_name.clone();

    let packet_rx = std::mem::replace(&mut session.packet_rx, mpsc::channel(1).1);
    let audio_rx = session.audio_rx.take();

    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<ControlMessage>(128);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true));
    let audio_muted = Arc::new(AtomicBool::new(false));
    let screen_off = Arc::new(AtomicBool::new(turn_screen_off));
    let hwnd_slot = Arc::new(AtomicIsize::new(0));
    let latest_frame: Arc<Mutex<Option<DecodedVideoFrame>>> = Arc::new(Mutex::new(None));
    let frame_w = Arc::new(AtomicU32::new(init_w));
    let frame_h = Arc::new(AtomicU32::new(init_h));

    if turn_screen_off {
        let _ = session
            .send_control(ControlMessage::SetDisplayPower { on: false })
            .await;
    }

    // Native WASAPI audio player + Media Foundation AAC/Opus decoder
    let audio_player = if let Some(rx) = audio_rx {
        match audio_codec.as_str() {
            "raw" => start_raw_player(rx, audio_buffer_ms, audio_muted.clone()),
            "aac" | "opus" => {
                let kind = if audio_codec == "opus" {
                    AudioCodecKind::Opus
                } else {
                    AudioCodecKind::Aac
                };
                if let Some(player) = start_pcm_player(audio_buffer_ms, audio_muted.clone()) {
                    let sink = player.sink();
                    let stop_audio = stop_flag.clone();
                    std::thread::Builder::new()
                        .name(format!("nexus-audio-{audio_codec}"))
                        .spawn(move || {
                            let Ok(_mf) = MfSession::start() else {
                                return;
                            };
                            let mut dec = match AudioMfDecoder::new(kind) {
                                Ok(d) => d,
                                Err(e) => {
                                    eprintln!("[nexus-mirror] Audio decoder init failed: {e}");
                                    return;
                                }
                            };
                            let mut rx = rx;
                            while !stop_audio.load(Ordering::Relaxed) {
                                let Some(pkt) = rx.blocking_recv() else {
                                    break;
                                };
                                match dec.decode_packet(&pkt.data, pkt.pts, pkt.is_config) {
                                    Ok(pcm) if !pcm.is_empty() => sink.push_s16le(&pcm),
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("[nexus-mirror] Audio frame decode warn: {e}");
                                    }
                                }
                            }
                        })
                        .ok();
                    Some(player)
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };

    // Dedicated control sender task using engine's session socket
    let ctrl_session_sender = session.control_sender();
    let stop_ctrl = stop_flag.clone();
    let ctrl_task = tauri::async_runtime::spawn(async move {
        let mut screen_power_on = !turn_screen_off;
        while !stop_ctrl.load(Ordering::Relaxed) {
            let Some(msg) = ctrl_rx.recv().await else {
                break;
            };
            if let ControlMessage::SetDisplayPower { on } = &msg {
                screen_power_on = *on;
            }
            let _ = ctrl_session_sender.send(msg).await;
        }
        if !screen_power_on {
            let _ = ctrl_session_sender
                .send(ControlMessage::SetDisplayPower { on: true })
                .await;
        }
    });

    // Dedicated Media Foundation H.264 decoder thread
    let stop_dec = stop_flag.clone();
    let latest_for_dec = latest_frame.clone();
    let hwnd_for_dec = hwnd_slot.clone();
    let fw_dec = frame_w.clone();
    let fh_dec = frame_h.clone();
    std::thread::Builder::new()
        .name("nexus-h264-decoder".to_string())
        .spawn(move || {
            let Ok(_mf) = MfSession::start() else {
                eprintln!("[nexus-mirror] MFStartup failed on video decoder thread");
                return;
            };
            let mut decoder = match H264MfDecoder::new(init_w, init_h) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[nexus-mirror] H264MfDecoder init failed: {e}");
                    return;
                }
            };
            let mut rx = packet_rx;
            while !stop_dec.load(Ordering::Relaxed) {
                let Some(pkt) = rx.blocking_recv() else {
                    break;
                };
                match decoder.decode_packet(&pkt.data, pkt.pts, pkt.is_config, pkt.is_key_frame) {
                    Ok(frames) => {
                        if let Some(frame) = frames.into_iter().last() {
                            fw_dec.store(frame.width.max(1), Ordering::Relaxed);
                            fh_dec.store(frame.height.max(1), Ordering::Relaxed);
                            if let Ok(mut guard) = latest_for_dec.lock() {
                                *guard = Some(frame);
                            }
                            let h = hwnd_for_dec.load(Ordering::Relaxed);
                            if h != 0 {
                                unsafe { PostMessageW(h as _, WM_APP_NEW_FRAME, 0, 0) };
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[nexus-mirror] Video decode warn: {e}");
                    }
                }
            }
            let _ = decoder.flush();
        })
        .map_err(|e| format!("Failed to spawn H264 decoder thread: {e}"))?;

    // Wait for native Win32 window + D3D11 SwapChain startup
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let hwnd_for_ui = hwnd_slot.clone();
    let latest_for_ui = latest_frame.clone();
    let stop_for_ui = stop_flag.clone();
    let running_for_ui = running.clone();
    let screen_off_for_ui = screen_off.clone();
    let ctrl_for_ui = ctrl_tx.clone();
    let app_for_ui = app.clone();
    let _ = max_fps; // Device server honors max_fps directly; D3D11 presents every decoded frame immediately

    let window_thread = std::thread::Builder::new()
        .name("nexus-mirror-win32".to_string())
        .spawn(move || {
            run_native_mirror_window(
                &device_name,
                init_w,
                init_h,
                hwnd_for_ui,
                latest_for_ui,
                stop_for_ui,
                running_for_ui,
                screen_off_for_ui,
                ctrl_for_ui,
                app_for_ui,
                ready_tx,
            );
        })
        .map_err(|e| format!("Failed to spawn native mirror window thread: {e}"))?;

    match ready_rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            stop_flag.store(true, Ordering::SeqCst);
            running.store(false, Ordering::SeqCst);
            ctrl_task.abort();
            let _ = window_thread.join();
            return Err(e);
        }
        Err(_) => {
            stop_flag.store(true, Ordering::SeqCst);
            running.store(false, Ordering::SeqCst);
            ctrl_task.abort();
            let _ = window_thread.join();
            return Err("Mirror window thread aborted during initialization".into());
        }
    }

    let handle = Arc::new(MirrorHandle {
        stop_flag,
        running,
        audio_muted,
        screen_off,
        hwnd: hwnd_slot,
        ctrl_tx,
        session: Mutex::new(Some(session)),
        audio_player: Mutex::new(audio_player),
        window_thread: Mutex::new(Some(window_thread)),
        tasks: Mutex::new(vec![ctrl_task]),
    });

    // If video stream closes naturally (USB unplugged / phone reboot), stop handle cleanly.
    let app_watch = app;
    let handle_weak = Arc::downgrade(&handle);
    std::thread::Builder::new()
        .name("nexus-mirror-watchdog".to_string())
        .spawn(move || {
            while let Some(strong) = handle_weak.upgrade() {
                if !strong.is_running() || strong.stop_flag.load(Ordering::Relaxed) {
                    strong.stop();
                    let _ = app_watch.emit("scrcpy-stopped", ());
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        })
        .ok();

    Ok(handle)
}

#[allow(clippy::too_many_arguments)]
fn run_native_mirror_window(
    device_name: &str,
    init_w: u32,
    init_h: u32,
    hwnd_slot: Arc<AtomicIsize>,
    latest_frame: Arc<Mutex<Option<DecodedVideoFrame>>>,
    stop_flag: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    screen_off: Arc<AtomicBool>,
    ctrl_tx: mpsc::Sender<ControlMessage>,
    app: AppHandle,
    ready_tx: tokio::sync::oneshot::Sender<Result<(), String>>,
) {
    let class_name = to_wide("NexusNativeMirrorClass");
    let title_str = if device_name.trim().is_empty() {
        "Nexus Screen Mirror".to_string()
    } else {
        format!("Nexus Screen Mirror ({})", device_name.trim())
    };
    let title = to_wide(&title_str);
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let (icon_big, icon_small) = load_exe_icons();
    let black_brush: HBRUSH = unsafe { CreateSolidBrush(0x0000_0000) };

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(mirror_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: icon_big,
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: black_brush,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: icon_small,
    };
    unsafe { RegisterClassExW(&wc) };

    let work = primary_monitor_work_rect();
    let (win_x, win_y, win_w, win_h) = compute_window_rect_for_video(
        init_w,
        init_h,
        work,
        None,
        WS_OVERLAPPEDWINDOW,
        WS_EX_APPWINDOW,
    );

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            win_x,
            win_y,
            win_w,
            win_h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        let _ = ready_tx.send(Err("CreateWindowExW failed for mirror window".into()));
        running.store(false, Ordering::SeqCst);
        return;
    }

    let dark: i32 = 1;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&dark as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        if !icon_big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon_big as isize);
        }
        if !icon_small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, icon_small as isize);
        }
    }

    let mut client: SYS_RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let client_w = (client.right - client.left).max(1) as u32;
    let client_h = (client.bottom - client.top).max(1) as u32;

    let renderer = match D3d11MirrorRenderer::new(hwnd as isize, client_w, client_h) {
        Ok(r) => r,
        Err(e) => {
            unsafe { DestroyWindow(hwnd) };
            let _ = ready_tx.send(Err(e));
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let state = Box::new(MirrorWindowState {
        hwnd,
        renderer,
        latest_frame,
        stop_flag: stop_flag.clone(),
        running: running.clone(),
        screen_off,
        ctrl_tx,
        video_w: init_w,
        video_h: init_h,
        dest_rect: RECT {
            left: 0,
            top: 0,
            right: client_w as i32,
            bottom: client_h as i32,
        },
        client_w,
        client_h,
        left_down: false,
        mouse_buttons: 0,
        is_fullscreen: false,
        saved_style: WS_OVERLAPPEDWINDOW,
        saved_placement: unsafe {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            wp
        },
    });
    let state_ptr = Box::into_raw(state);
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };

    hwnd_slot.store(hwnd as isize, Ordering::SeqCst);
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
    }
    let _ = ready_tx.send(Ok(()));

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    hwnd_slot.store(0, Ordering::SeqCst);
    running.store(false, Ordering::SeqCst);
    stop_flag.store(true, Ordering::SeqCst);
    let _ = app.emit("scrcpy-stopped", ());

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(state_ptr));
    }
}

struct MirrorWindowState {
    hwnd: windows_sys::Win32::Foundation::HWND,
    renderer: D3d11MirrorRenderer,
    latest_frame: Arc<Mutex<Option<DecodedVideoFrame>>>,
    stop_flag: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    screen_off: Arc<AtomicBool>,
    ctrl_tx: mpsc::Sender<ControlMessage>,
    video_w: u32,
    video_h: u32,
    client_w: u32,
    client_h: u32,
    dest_rect: RECT,
    left_down: bool,
    mouse_buttons: u32,
    is_fullscreen: bool,
    saved_style: u32,
    saved_placement: WINDOWPLACEMENT,
}

impl MirrorWindowState {
    fn update_dest_rect(&mut self) {
        self.dest_rect = compute_letterbox_rect(self.video_w, self.video_h, self.client_w, self.client_h);
    }

    fn handle_new_frame(&mut self) {
        let frame = {
            let Ok(mut g) = self.latest_frame.lock() else {
                return;
            };
            g.take()
        };
        let Some(frame) = frame else {
            return;
        };

        if frame.width != self.video_w || frame.height != self.video_h {
            self.video_w = frame.width;
            self.video_h = frame.height;
            self.resize_for_orientation(frame.width, frame.height);
        }
        self.update_dest_rect();
        if let Err(e) = self.renderer.render_nv12(&frame, self.dest_rect, self.client_w, self.client_h) {
            eprintln!("[nexus-mirror] D3D11 present warn: {e}");
        }
    }

    fn resize_for_orientation(&mut self, new_w: u32, new_h: u32) {
        if self.is_fullscreen || unsafe { IsZoomed(self.hwnd) } != 0 {
            self.update_dest_rect();
            return;
        }
        let mut cur_rect: SYS_RECT = unsafe { std::mem::zeroed() };
        unsafe { GetWindowRect(self.hwnd, &mut cur_rect) };
        let cur_center = (
            (cur_rect.left + cur_rect.right) / 2,
            (cur_rect.top + cur_rect.bottom) / 2,
        );
        let work = monitor_work_rect_for_window(self.hwnd);
        let style = unsafe { GetWindowLongPtrW(self.hwnd, GWL_STYLE) } as u32;
        let (x, y, w, h) =
            compute_window_rect_for_video(new_w, new_h, work, Some(cur_center), style, WS_EX_APPWINDOW);
        unsafe {
            SetWindowPos(
                self.hwnd,
                std::ptr::null_mut(),
                x,
                y,
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let mut client: SYS_RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(self.hwnd, &mut client) };
        self.client_w = (client.right - client.left).max(1) as u32;
        self.client_h = (client.bottom - client.top).max(1) as u32;
        let _ = self.renderer.resize(self.client_w, self.client_h);
        self.update_dest_rect();
    }

    fn toggle_fullscreen(&mut self) {
        unsafe {
            if !self.is_fullscreen {
                self.saved_style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
                self.saved_placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                GetWindowPlacement(self.hwnd, &mut self.saved_placement);
                let mon = monitor_full_rect_for_window(self.hwnd);
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE) as isize);
                SetWindowPos(
                    self.hwnd,
                    HWND_TOP,
                    mon.left,
                    mon.top,
                    mon.right - mon.left,
                    mon.bottom - mon.top,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
                self.is_fullscreen = true;
            } else {
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, self.saved_style as isize);
                SetWindowPlacement(self.hwnd, &self.saved_placement);
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
                self.is_fullscreen = false;
            }
        }
    }

    fn map_client_to_phone(&self, x: i32, y: i32) -> Position {
        let dw = (self.dest_rect.right - self.dest_rect.left).max(1);
        let dh = (self.dest_rect.bottom - self.dest_rect.top).max(1);
        let cx = x.clamp(self.dest_rect.left, self.dest_rect.right - 1) - self.dest_rect.left;
        let cy = y.clamp(self.dest_rect.top, self.dest_rect.bottom - 1) - self.dest_rect.top;
        let px = ((cx as i64 * self.video_w as i64) / dw as i64)
            .clamp(0, (self.video_w as i64 - 1).max(0)) as i32;
        let py = ((cy as i64 * self.video_h as i64) / dh as i64)
            .clamp(0, (self.video_h as i64 - 1).max(0)) as i32;
        Position {
            x: px,
            y: py,
            screen_width: self.video_w.min(u16::MAX as u32) as u16,
            screen_height: self.video_h.min(u16::MAX as u32) as u16,
        }
    }

    fn send_touch(&self, action: u8, x: i32, y: i32, pressure: f32, action_button: u32) {
        let position = self.map_client_to_phone(x, y);
        let _ = self.ctrl_tx.try_send(ControlMessage::InjectTouchEvent {
            action,
            pointer_id: POINTER_ID_MOUSE,
            position,
            pressure,
            action_button,
            buttons: self.mouse_buttons,
        });
    }
}

const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOSIZE: u32 = 0x0001;

unsafe extern "system" fn mirror_wnd_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut MirrorWindowState;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *ptr;

    match msg {
        WM_APP_NEW_FRAME => {
            state.handle_new_frame();
            0
        }
        WM_APP_STOP => {
            DestroyWindow(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            // Re-present last frame if window invalidated, otherwise validate client rect.
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_SIZE => {
            let cw = (lparam & 0xffff) as u32;
            let ch = ((lparam >> 16) & 0xffff) as u32;
            if cw > 0 && ch > 0 {
                state.client_w = cw;
                state.client_h = ch;
                let _ = state.renderer.resize(cw, ch);
                state.update_dest_rect();
                if let Err(e) = state.renderer.redraw_last(state.dest_rect, cw, ch) {
                    eprintln!("[nexus-mirror] Resize redraw warn: {e}");
                }
            }
            0
        }
        WM_LBUTTONDOWN => {
            SetCapture(hwnd);
            state.left_down = true;
            state.mouse_buttons |= AMOTION_EVENT_BUTTON_PRIMARY;
            let (x, y) = lp_coords(lparam);
            state.send_touch(
                AMOTION_EVENT_ACTION_DOWN,
                x,
                y,
                1.0,
                AMOTION_EVENT_BUTTON_PRIMARY,
            );
            0
        }
        WM_MOUSEMOVE => {
            if state.left_down {
                let (x, y) = lp_coords(lparam);
                state.send_touch(AMOTION_EVENT_ACTION_MOVE, x, y, 1.0, 0);
            }
            0
        }
        WM_LBUTTONUP => {
            if state.left_down {
                state.left_down = false;
                state.mouse_buttons &= !AMOTION_EVENT_BUTTON_PRIMARY;
                let (x, y) = lp_coords(lparam);
                state.send_touch(
                    AMOTION_EVENT_ACTION_UP,
                    x,
                    y,
                    0.0,
                    AMOTION_EVENT_BUTTON_PRIMARY,
                );
                ReleaseCapture();
            }
            0
        }
        WM_RBUTTONDOWN => {
            state.mouse_buttons |= AMOTION_EVENT_BUTTON_SECONDARY;
            let _ = state.ctrl_tx.try_send(ControlMessage::BackOrScreenOn {
                action: AKEY_EVENT_ACTION_DOWN,
            });
            0
        }
        WM_RBUTTONUP => {
            state.mouse_buttons &= !AMOTION_EVENT_BUTTON_SECONDARY;
            let _ = state.ctrl_tx.try_send(ControlMessage::BackOrScreenOn {
                action: AKEY_EVENT_ACTION_UP,
            });
            0
        }
        WM_MBUTTONDOWN => {
            state.mouse_buttons |= AMOTION_EVENT_BUTTON_TERTIARY;
            let _ = state.ctrl_tx.try_send(ControlMessage::InjectKeycode {
                action: AKEY_EVENT_ACTION_DOWN,
                keycode: AKEYCODE_HOME,
                repeat: 0,
                metastate: 0,
            });
            0
        }
        WM_MBUTTONUP => {
            state.mouse_buttons &= !AMOTION_EVENT_BUTTON_TERTIARY;
            let _ = state.ctrl_tx.try_send(ControlMessage::InjectKeycode {
                action: AKEY_EVENT_ACTION_UP,
                keycode: AKEYCODE_HOME,
                repeat: 0,
                metastate: 0,
            });
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) as u16) as i16 as f32 / WHEEL_DELTA as f32;
            let mut pt = SYS_POINT {
                x: ((lparam & 0xffff) as u16) as i16 as i32,
                y: (((lparam >> 16) & 0xffff) as u16) as i16 as i32,
            };
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
            let _ = state.ctrl_tx.try_send(ControlMessage::InjectScrollEvent {
                position: state.map_client_to_phone(pt.x, pt.y),
                hscroll: 0.0,
                vscroll: delta.clamp(-16.0, 16.0),
                buttons: state.mouse_buttons,
            });
            0
        }
        WM_MOUSEHWHEEL => {
            let delta = ((wparam >> 16) as u16) as i16 as f32 / WHEEL_DELTA as f32;
            let mut pt = SYS_POINT {
                x: ((lparam & 0xffff) as u16) as i16 as i32,
                y: (((lparam >> 16) & 0xffff) as u16) as i16 as i32,
            };
            windows_sys::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
            let _ = state.ctrl_tx.try_send(ControlMessage::InjectScrollEvent {
                position: state.map_client_to_phone(pt.x, pt.y),
                hscroll: (-delta).clamp(-16.0, 16.0),
                vscroll: 0.0,
                buttons: state.mouse_buttons,
            });
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wparam as u16;
            let is_repeat = (lparam & (1 << 30)) != 0;
            let alt = (GetKeyState(VK_MENU as i32) & -0x8000i16) != 0;
            let shift = (GetKeyState(VK_SHIFT as i32) & -0x8000i16) != 0;
            let ctrl = (GetKeyState(VK_CONTROL as i32) & -0x8000i16) != 0;

            if !is_repeat && (vk == VK_F11 || (alt && vk == b'F' as u16)) {
                state.toggle_fullscreen();
                return 0;
            }
            if !is_repeat && alt && !ctrl {
                match vk {
                    k if k == b'O' as u16 => {
                        let on = shift;
                        state.screen_off.store(!on, Ordering::Relaxed);
                        let _ = state.ctrl_tx.try_send(ControlMessage::SetDisplayPower { on });
                        return 0;
                    }
                    k if k == b'R' as u16 => {
                        let _ = state.ctrl_tx.try_send(ControlMessage::RotateDevice);
                        return 0;
                    }
                    k if k == b'H' as u16 => {
                        send_click_keycode(&state.ctrl_tx, AKEYCODE_HOME);
                        return 0;
                    }
                    k if k == b'B' as u16 => {
                        send_click_keycode(&state.ctrl_tx, AKEYCODE_BACK);
                        return 0;
                    }
                    k if k == b'S' as u16 => {
                        send_click_keycode(&state.ctrl_tx, AKEYCODE_APP_SWITCH);
                        return 0;
                    }
                    k if k == b'P' as u16 => {
                        send_click_keycode(&state.ctrl_tx, AKEYCODE_POWER);
                        return 0;
                    }
                    k if k == b'N' as u16 => {
                        let msg = if shift {
                            ControlMessage::CollapsePanels
                        } else {
                            ControlMessage::ExpandNotificationPanel
                        };
                        let _ = state.ctrl_tx.try_send(msg);
                        return 0;
                    }
                    _ => {}
                }
            }

            if let Some(keycode) = map_vk_to_android(vk, ctrl) {
                let meta = build_android_meta(shift, ctrl, alt);
                let _ = state.ctrl_tx.try_send(ControlMessage::InjectKeycode {
                    action: AKEY_EVENT_ACTION_DOWN,
                    keycode,
                    repeat: u32::from(is_repeat),
                    metastate: meta,
                });
            }
            0
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let vk = wparam as u16;
            let alt = (GetKeyState(VK_MENU as i32) & -0x8000i16) != 0;
            let shift = (GetKeyState(VK_SHIFT as i32) & -0x8000i16) != 0;
            let ctrl = (GetKeyState(VK_CONTROL as i32) & -0x8000i16) != 0;
            if let Some(keycode) = map_vk_to_android(vk, ctrl) {
                let meta = build_android_meta(shift, ctrl, alt);
                let _ = state.ctrl_tx.try_send(ControlMessage::InjectKeycode {
                    action: AKEY_EVENT_ACTION_UP,
                    keycode,
                    repeat: 0,
                    metastate: meta,
                });
            }
            0
        }
        WM_CHAR => {
            let code = wparam as u32;
            let ctrl = (GetKeyState(VK_CONTROL as i32) & -0x8000i16) != 0;
            let alt = (GetKeyState(VK_MENU as i32) & -0x8000i16) != 0;
            if !ctrl && !alt && code >= 0x20 {
                if let Some(ch) = char::from_u32(code) {
                    let _ = state.ctrl_tx.try_send(ControlMessage::InjectText {
                        text: ch.to_string(),
                    });
                }
            }
            0
        }
        WM_CLOSE => {
            if state.screen_off.swap(false, Ordering::SeqCst) {
                let _ = state
                    .ctrl_tx
                    .try_send(ControlMessage::SetDisplayPower { on: true });
            }
            state.stop_flag.store(true, Ordering::SeqCst);
            state.running.store(false, Ordering::SeqCst);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn send_click_keycode(tx: &mpsc::Sender<ControlMessage>, keycode: u32) {
    let _ = tx.try_send(ControlMessage::InjectKeycode {
        action: AKEY_EVENT_ACTION_DOWN,
        keycode,
        repeat: 0,
        metastate: 0,
    });
    let _ = tx.try_send(ControlMessage::InjectKeycode {
        action: AKEY_EVENT_ACTION_UP,
        keycode,
        repeat: 0,
        metastate: 0,
    });
}

fn lp_coords(lparam: LPARAM) -> (i32, i32) {
    let x = ((lparam & 0xffff) as u16) as i16 as i32;
    let y = (((lparam >> 16) & 0xffff) as u16) as i16 as i32;
    (x, y)
}

fn build_android_meta(shift: bool, ctrl: bool, alt: bool) -> u32 {
    let mut m = 0u32;
    if shift {
        m |= AMETA_SHIFT_ON;
    }
    if ctrl {
        m |= AMETA_CTRL_ON;
    }
    if alt {
        m |= AMETA_ALT_ON;
    }
    m
}

fn map_vk_to_android(vk: u16, ctrl: bool) -> Option<u32> {
    match vk {
        VK_BACK => Some(AKEYCODE_DEL),
        VK_TAB => Some(AKEYCODE_TAB),
        VK_RETURN => Some(AKEYCODE_ENTER),
        VK_ESCAPE => Some(AKEYCODE_ESCAPE),
        VK_SPACE if ctrl => Some(AKEYCODE_SPACE),
        VK_PRIOR => Some(AKEYCODE_PAGE_UP),
        VK_NEXT => Some(AKEYCODE_PAGE_DOWN),
        VK_END => Some(AKEYCODE_MOVE_END),
        VK_HOME => Some(AKEYCODE_MOVE_HOME),
        VK_LEFT => Some(AKEYCODE_DPAD_LEFT),
        VK_UP => Some(AKEYCODE_DPAD_UP),
        VK_RIGHT => Some(AKEYCODE_DPAD_RIGHT),
        VK_DOWN => Some(AKEYCODE_DPAD_DOWN),
        VK_DELETE => Some(AKEYCODE_FORWARD_DEL),
        v if ctrl && (b'A' as u16..=b'Z' as u16).contains(&v) => {
            Some(AKEYCODE_A + (v - b'A' as u16) as u32)
        }
        v if ctrl && (b'0' as u16..=b'9' as u16).contains(&v) => {
            Some(AKEYCODE_0 + (v - b'0' as u16) as u32)
        }
        v if (VK_F1..=VK_F12).contains(&v) => Some(AKEYCODE_F1 + (v - VK_F1) as u32),
        0xAF => Some(AKEYCODE_VOLUME_UP),
        0xAE => Some(AKEYCODE_VOLUME_DOWN),
        0xBA if ctrl => Some(AKEYCODE_SEMICOLON),
        0xBB if ctrl => Some(AKEYCODE_EQUALS),
        0xBD if ctrl => Some(AKEYCODE_MINUS),
        0xBF if ctrl => Some(AKEYCODE_SLASH),
        0xC0 if ctrl => Some(AKEYCODE_GRAVE),
        0xDB if ctrl => Some(AKEYCODE_LEFT_BRACKET),
        0xDC if ctrl => Some(AKEYCODE_BACKSLASH),
        0xDD if ctrl => Some(AKEYCODE_RIGHT_BRACKET),
        0xDE if ctrl => Some(AKEYCODE_APOSTROPHE),
        _ => None,
    }
}

struct D3d11MirrorRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    vp_enum: Option<ID3D11VideoProcessorEnumerator>,
    vp: Option<ID3D11VideoProcessor>,
    output_view: Option<ID3D11VideoProcessorOutputView>,
    nv12_tex: Option<ID3D11Texture2D>,
    nv12_view: Option<ID3D11VideoProcessorInputView>,
    coded_w: u32,
    coded_h: u32,
    last_frame: Option<DecodedVideoFrame>,
}

impl D3d11MirrorRenderer {
    fn new(hwnd_raw: isize, client_w: u32, client_h: u32) -> Result<Self, String> {
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        let levels = [
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
        ];

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;

        let hr = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if hr.is_err() {
            unsafe {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    HMODULE::default(),
                    flags,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
            }
            .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;
        }

        let device = device.ok_or_else(|| "D3D11CreateDevice returned null device".to_string())?;
        let context =
            context.ok_or_else(|| "D3D11CreateDevice returned null context".to_string())?;

        let video_device: ID3D11VideoDevice = device
            .cast()
            .map_err(|e| format!("ID3D11VideoDevice unavailable on GPU: {e}"))?;
        let video_context: ID3D11VideoContext = context
            .cast()
            .map_err(|e| format!("ID3D11VideoContext unavailable: {e}"))?;

        let factory: IDXGIFactory2 =
            unsafe { CreateDXGIFactory1() }.map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        unsafe {
            let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);
        }

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: client_w.max(1),
            Height: client_h.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_NONE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let swap_chain = unsafe { factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None) }
            .map_err(|e| format!("CreateSwapChainForHwnd failed: {e}"))?;

        let mut renderer = Self {
            device,
            context,
            swap_chain,
            rtv: None,
            video_device,
            video_context,
            vp_enum: None,
            vp: None,
            output_view: None,
            nv12_tex: None,
            nv12_view: None,
            coded_w: 0,
            coded_h: 0,
            last_frame: None,
        };
        renderer.rebuild_backbuffer_views()?;
        Ok(renderer)
    }

    fn rebuild_backbuffer_views(&mut self) -> Result<(), String> {
        self.output_view = None;
        self.rtv = None;

        let backbuffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0) }
            .map_err(|e| format!("SwapChain GetBuffer failed: {e}"))?;
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&backbuffer, None, Some(&mut rtv))
                .map_err(|e| format!("CreateRenderTargetView failed: {e}"))?;
        }
        self.rtv = rtv;

        if let Some(vp_enum) = &self.vp_enum {
            let ov_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: Default::default(),
            };
            let res: ID3D11Resource = backbuffer
                .cast()
                .map_err(|e| format!("Backbuffer Resource cast: {e}"))?;
            let mut ov: Option<ID3D11VideoProcessorOutputView> = None;
            unsafe {
                self.video_device
                    .CreateVideoProcessorOutputView(&res, vp_enum, &ov_desc, Some(&mut ov))
                    .map_err(|e| format!("CreateVideoProcessorOutputView failed: {e}"))?;
            }
            self.output_view = ov;
        }
        Ok(())
    }

    fn ensure_video_processor(
        &mut self,
        coded_w: u32,
        coded_h: u32,
        out_w: u32,
        out_h: u32,
    ) -> Result<(), String> {
        if self.nv12_tex.is_some()
            && self.coded_w == coded_w
            && self.coded_h == coded_h
            && self.vp.is_some()
            && self.output_view.is_some()
        {
            return Ok(());
        }

        if self.vp.is_none() || self.coded_w != coded_w || self.coded_h != coded_h {
            self.nv12_view = None;
            self.nv12_tex = None;
            self.output_view = None;
            self.vp = None;
            self.vp_enum = None;

            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: coded_w,
                InputHeight: coded_h,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: out_w.max(1),
                OutputHeight: out_h.max(1),
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let vp_enum = unsafe { self.video_device.CreateVideoProcessorEnumerator(&content) }
                .map_err(|e| format!("CreateVideoProcessorEnumerator: {e}"))?;
            let vp = unsafe { self.video_device.CreateVideoProcessor(&vp_enum, 0) }
                .map_err(|e| format!("CreateVideoProcessor: {e}"))?;

            let tex_desc = D3D11_TEXTURE2D_DESC {
                Width: coded_w,
                Height: coded_h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: 0,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut tex: Option<ID3D11Texture2D> = None;
            unsafe {
                self.device
                    .CreateTexture2D(&tex_desc, None, Some(&mut tex))
                    .map_err(|e| format!("CreateTexture2D NV12 failed: {e}"))?;
            }
            let tex = tex.ok_or_else(|| "NV12 texture null".to_string())?;
            let tex_res: ID3D11Resource = tex
                .cast()
                .map_err(|e| format!("NV12 texture Resource cast: {e}"))?;

            let iv_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: Default::default(),
            };
            let mut iv: Option<ID3D11VideoProcessorInputView> = None;
            unsafe {
                self.video_device
                    .CreateVideoProcessorInputView(&tex_res, &vp_enum, &iv_desc, Some(&mut iv))
                    .map_err(|e| format!("CreateVideoProcessorInputView failed: {e}"))?;
            }

            self.vp_enum = Some(vp_enum);
            self.vp = Some(vp);
            self.nv12_tex = Some(tex);
            self.nv12_view = iv;
            self.coded_w = coded_w;
            self.coded_h = coded_h;
        }

        if self.output_view.is_none() {
            self.rebuild_backbuffer_views()?;
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.output_view = None;
        self.rtv = None;
        unsafe {
            self.swap_chain
                .ResizeBuffers(
                    0,
                    width.max(1),
                    height.max(1),
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .map_err(|e| format!("ResizeBuffers failed: {e}"))?;
        }
        self.rebuild_backbuffer_views()
    }

    fn render_nv12(
        &mut self,
        frame: &DecodedVideoFrame,
        dest_rect: RECT,
        client_w: u32,
        client_h: u32,
    ) -> Result<(), String> {
        self.ensure_video_processor(frame.coded_width, frame.coded_height, client_w, client_h)?;

        if let Some(tex) = &self.nv12_tex {
            unsafe {
                self.context.UpdateSubresource(
                    tex,
                    0,
                    None,
                    frame.nv12_data.as_ptr().cast(),
                    frame.stride,
                    frame.stride.saturating_mul(frame.coded_height),
                );
            }
        }
        self.last_frame = Some(frame.clone());
        self.present_current(frame, dest_rect, client_w, client_h)
    }

    fn redraw_last(&self, dest_rect: RECT, client_w: u32, client_h: u32) -> Result<(), String> {
        if let Some(frame) = &self.last_frame {
            self.present_current(frame, dest_rect, client_w, client_h)
        } else {
            if let Some(rtv) = &self.rtv {
                unsafe {
                    self.context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);
                    self.swap_chain
                        .Present(0, DXGI_PRESENT(0))
                        .ok()
                        .map_err(|e| format!("Present clear failed: {e}"))?;
                }
            }
            Ok(())
        }
    }

    fn present_current(
        &self,
        frame: &DecodedVideoFrame,
        dest_rect: RECT,
        client_w: u32,
        client_h: u32,
    ) -> Result<(), String> {
        let (Some(vp), Some(iv), Some(ov)) = (&self.vp, &self.nv12_view, &self.output_view) else {
            return Ok(());
        };
        if let Some(rtv) = &self.rtv {
            unsafe {
                self.context
                    .ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);
            }
        }

        let src_rect = RECT {
            left: frame.crop_x as i32,
            top: frame.crop_y as i32,
            right: (frame.crop_x + frame.width) as i32,
            bottom: (frame.crop_y + frame.height) as i32,
        };
        let target_rect = RECT {
            left: 0,
            top: 0,
            right: client_w as i32,
            bottom: client_h as i32,
        };

        let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: std::ptr::null_mut(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(iv.clone())),
            ppFutureSurfaces: std::ptr::null_mut(),
            ppPastSurfacesRight: std::ptr::null_mut(),
            pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
            ppFutureSurfacesRight: std::ptr::null_mut(),
        }];

        let res = unsafe {
            self.video_context
                .VideoProcessorSetStreamSourceRect(vp, 0, true, Some(&src_rect));
            self.video_context
                .VideoProcessorSetStreamDestRect(vp, 0, true, Some(&dest_rect));
            self.video_context
                .VideoProcessorSetOutputTargetRect(vp, true, Some(&target_rect));
            self.video_context.VideoProcessorBlt(vp, ov, 0, &streams)
        };

        let _ = unsafe { std::mem::ManuallyDrop::take(&mut streams[0].pInputSurface) };
        let _ = unsafe { std::mem::ManuallyDrop::take(&mut streams[0].pInputSurfaceRight) };

        res.map_err(|e| format!("VideoProcessorBlt failed: {e}"))?;
        unsafe {
            self.swap_chain
                .Present(0, DXGI_PRESENT(0))
                .ok()
                .map_err(|e| format!("SwapChain Present failed: {e}"))?;
        }
        Ok(())
    }
}

fn compute_letterbox_rect(video_w: u32, video_h: u32, client_w: u32, client_h: u32) -> RECT {
    let vw = video_w.max(1) as f64;
    let vh = video_h.max(1) as f64;
    let cw = client_w.max(1) as f64;
    let ch = client_h.max(1) as f64;

    let scale = (cw / vw).min(ch / vh);
    let draw_w = (vw * scale).round().max(1.0) as i32;
    let draw_h = (vh * scale).round().max(1.0) as i32;
    let left = ((client_w as i32 - draw_w) / 2).max(0);
    let top = ((client_h as i32 - draw_h) / 2).max(0);
    RECT {
        left,
        top,
        right: left + draw_w,
        bottom: top + draw_h,
    }
}

fn compute_window_rect_for_video(
    video_w: u32,
    video_h: u32,
    work: SYS_RECT,
    center: Option<(i32, i32)>,
    style: u32,
    ex_style: u32,
) -> (i32, i32, i32, i32) {
    let work_w = (work.right - work.left).max(400) as f64;
    let work_h = (work.bottom - work.top).max(400) as f64;
    let max_cw = work_w * 0.85;
    let max_ch = work_h * 0.85;

    let vw = video_w.max(1) as f64;
    let vh = video_h.max(1) as f64;
    let scale = (max_cw / vw).min(max_ch / vh).min(1.0);
    let client_w = (vw * scale).round().max(240.0) as i32;
    let client_h = (vh * scale).round().max(240.0) as i32;

    let mut wr = SYS_RECT {
        left: 0,
        top: 0,
        right: client_w,
        bottom: client_h,
    };
    unsafe { AdjustWindowRectEx(&mut wr, style, 0, ex_style) };
    let total_w = wr.right - wr.left;
    let total_h = wr.bottom - wr.top;

    let (cx, cy) = center.unwrap_or(((work.left + work.right) / 2, (work.top + work.bottom) / 2));
    let x = (cx - total_w / 2).clamp(work.left, (work.right - total_w).max(work.left));
    let y = (cy - total_h / 2).clamp(work.top, (work.bottom - total_h).max(work.top));
    (x, y, total_w, total_h)
}

fn monitor_info_for_handle(hmon: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<MONITORINFO> {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(hmon, &mut info) } != 0 {
        Some(info)
    } else {
        None
    }
}

fn primary_monitor_work_rect() -> SYS_RECT {
    let hmon = unsafe { MonitorFromPoint(SYS_POINT { x: 0, y: 0 }, MONITOR_DEFAULTTONEAREST) };
    monitor_info_for_handle(hmon)
        .map(|i| i.rcWork)
        .unwrap_or(SYS_RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        })
}

fn monitor_work_rect_for_window(hwnd: windows_sys::Win32::Foundation::HWND) -> SYS_RECT {
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitor_info_for_handle(hmon)
        .map(|i| i.rcWork)
        .unwrap_or_else(primary_monitor_work_rect)
}

fn monitor_full_rect_for_window(hwnd: windows_sys::Win32::Foundation::HWND) -> SYS_RECT {
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitor_info_for_handle(hmon)
        .map(|i| i.rcMonitor)
        .unwrap_or_else(primary_monitor_work_rect)
}

fn load_exe_icons() -> (
    windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) {
    let Ok(exe) = std::env::current_exe() else {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    };
    let wpath = to_wide(&exe.to_string_lossy());
    let mut big = std::ptr::null_mut();
    let mut small = std::ptr::null_mut();
    let count = unsafe { ExtractIconExW(wpath.as_ptr(), 0, &mut big, &mut small, 1) };
    if count == 0 {
        (std::ptr::null_mut(), std::ptr::null_mut())
    } else {
        (big, small)
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
