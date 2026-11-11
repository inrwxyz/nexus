// cs/trigger_pill.rs — Optional 0-MB-overhead native Win32 On-Screen Trigger Pill (compact icon button).
// Docked on the right side just above the bottom-right corner (~80.5% down, above system tray).
// Static/event-driven repaint only (0% idle CPU); completely destroyed when turned OFF.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDeviceCaps,
    GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, LOGPIXELSX, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, GetWindowRect, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetCursor, SetTimer, ShowWindow, SystemParametersInfoW, TranslateMessage,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, IDC_HAND, MSG, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WM_APP_VISIBILITY: u32 = WM_APP + 2;
const HOVER_TIMER_ID: usize = 1;

const HANDLE_SIZE: f32 = 40.0;
const MARGIN: f32 = 12.0;
const WIN_W: f32 = HANDLE_SIZE + MARGIN * 2.0;
const WIN_H: f32 = HANDLE_SIZE + MARGIN * 2.0;

struct PillRuntime {
    desired_enabled: bool,
    temp_hidden: bool,
    hwnd: isize,
    hovered: bool,
    scale: f32,
    on_trigger: Option<Arc<dyn Fn() + Send + Sync>>,
}

fn runtime() -> &'static Mutex<PillRuntime> {
    static RT: OnceLock<Mutex<PillRuntime>> = OnceLock::new();
    RT.get_or_init(|| {
        Mutex::new(PillRuntime {
            desired_enabled: false,
            temp_hidden: false,
            hwnd: 0,
            hovered: false,
            scale: 1.0,
            on_trigger: None,
        })
    })
}

pub fn set_enabled(enabled: bool, on_trigger: Arc<dyn Fn() + Send + Sync>) {
    let (hwnd, running) = {
        let mut rt = runtime().lock().unwrap();
        rt.desired_enabled = enabled;
        rt.on_trigger = Some(on_trigger);
        if !enabled {
            rt.temp_hidden = false;
            rt.hovered = false;
        }
        (rt.hwnd, rt.hwnd != 0)
    };

    if enabled && !running {
        spawn_pill_thread();
    } else if !enabled && hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
    } else if enabled && hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_APP_VISIBILITY, 1, 0);
        }
    }
}

/// Temporarily hides or restores the icon pill around BitBlt captures so it doesn't capture itself.
pub fn set_capture_hidden(hidden: bool) {
    let (hwnd, enabled) = {
        let mut rt = runtime().lock().unwrap();
        rt.temp_hidden = hidden;
        (rt.hwnd, rt.desired_enabled)
    };

    if hwnd != 0 && enabled {
        unsafe {
            PostMessageW(
                hwnd as HWND,
                WM_APP_VISIBILITY,
                if hidden { 0 } else { 1 },
                0,
            );
        }
    }
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    (pax - bax * h).hypot(pay - bay * h)
}

fn smoothstep(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Places the compact circular trigger icon cleanly on the lower-right desktop edge (~80.5% down),
/// right above the bottom-right corner and clear of the taskbar/tray.
pub fn compute_pill_origin(
    work: RECT,
    monitor_right: i32,
    monitor_bottom: i32,
    ww: i32,
    wh: i32,
    s: f32,
) -> (i32, i32) {
    let margin_px = (MARGIN * s).round() as i32;
    let right_anchor = work.right.min(monitor_right);
    let edge_pad = (14.0 * s).round() as i32;
    let x = right_anchor - edge_pad + margin_px - ww;

    let target_cy = ((monitor_bottom as f32) * 0.805).round() as i32;
    let min_y = work.top + (48.0 * s).round() as i32;
    let max_y = (work.bottom.min(monitor_bottom) - (110.0 * s).round() as i32)
        .min(monitor_bottom - (130.0 * s).round() as i32)
        + margin_px
        - wh;
    let y = (target_cy - wh / 2).clamp(min_y, max_y.max(min_y));
    (x, y)
}

unsafe fn render_handle(hwnd: HWND, hovered: bool, s: f32) {
    let ww = (WIN_W * s).round() as i32;
    let wh = (WIN_H * s).round() as i32;

    let mut work: RECT = std::mem::zeroed();
    SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        &mut work as *mut RECT as *mut std::ffi::c_void,
        0,
    );
    let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let got_mi = GetMonitorInfoW(hmon, &mut mi) != 0;
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    let monitor_right = if got_mi && mi.rcMonitor.right > 0 {
        mi.rcMonitor.right
    } else if screen_w > 0 {
        screen_w
    } else {
        work.right
    };
    let monitor_bottom = if got_mi && mi.rcMonitor.bottom > 0 {
        mi.rcMonitor.bottom
    } else if screen_h > 0 {
        screen_h
    } else {
        work.bottom
    };

    let (x, y) = compute_pill_origin(work, monitor_right, monitor_bottom, ww, wh, s);

    let screen = GetDC(std::ptr::null_mut());
    let mem = CreateCompatibleDC(screen);

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = ww;
    bmi.bmiHeader.biHeight = -wh;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
    if !dib.is_null() && !bits.is_null() {
        let old = SelectObject(mem, dib as *mut _);
        let buf = std::slice::from_raw_parts_mut(bits as *mut u32, (ww * wh) as usize);

        let cx = ww as f32 * 0.5;
        let cy = wh as f32 * 0.5;
        let radius = (if hovered { HANDLE_SIZE + 2.0 } else { HANDLE_SIZE } * 0.5) * s;

        let border_col = if hovered {
            [0.24, 0.88, 0.98]
        } else {
            [0.45, 0.65, 0.76]
        };
        let accent = if hovered {
            [0.35, 0.92, 1.0]
        } else {
            [0.06, 0.74, 0.86]
        };

        // Magnifying lens / circle-search icon geometry centered inside circle button
        let lens_cx = cx - 1.4 * s;
        let lens_cy = cy - 1.4 * s;
        let lens_r = 6.2 * s;
        let stroke = 1.05 * s;

        for py_i in 0..wh {
            for px_i in 0..ww {
                let px = px_i as f32 + 0.5;
                let py = py_i as f32 + 0.5;
                let d = (px - cx).hypot(py - cy) - radius;

                let mut pr = 0.0f32;
                let mut pg = 0.0f32;
                let mut pb = 0.0f32;
                let mut pa = 0.0f32;

                // Soft circular drop shadow
                if d > 0.0 {
                    let sh = ((-d * d) / (2.0 * 4.4 * s * 4.4 * s)).exp()
                        * if hovered { 0.44 } else { 0.34 };
                    pa = sh;
                }

                // Graphite glass circle body
                let fill_cov = smoothstep(0.6 - d);
                if fill_cov > 0.0 {
                    let br = if hovered { 0.11 } else { 0.08 };
                    let bg = if hovered { 0.14 } else { 0.10 };
                    let bb = if hovered { 0.19 } else { 0.13 };
                    let ba = (if hovered { 0.96 } else { 0.88 }) * fill_cov;
                    pr = pr * (1.0 - ba) + br * ba;
                    pg = pg * (1.0 - ba) + bg * ba;
                    pb = pb * (1.0 - ba) + bb * ba;
                    pa = pa * (1.0 - ba) + ba;
                }

                // Hairline glass border
                let rim_cov = smoothstep(1.2 - d.abs()) * if hovered { 0.78 } else { 0.38 };
                if rim_cov > 0.0 {
                    pr = pr * (1.0 - rim_cov) + border_col[0] * rim_cov;
                    pg = pg * (1.0 - rim_cov) + border_col[1] * rim_cov;
                    pb = pb * (1.0 - rim_cov) + border_col[2] * rim_cov;
                    pa = pa.max(rim_cov);
                }

                // Centered Circle-to-Search lens ring + handle + tiny center dot
                let d_ring = ((px - lens_cx).hypot(py - lens_cy) - lens_r).abs() - stroke;
                let d_stem = sd_segment(
                    px,
                    py,
                    lens_cx + 4.3 * s,
                    lens_cy + 4.3 * s,
                    lens_cx + 8.6 * s,
                    lens_cy + 8.6 * s,
                ) - stroke;
                let d_dot = (px - lens_cx).hypot(py - lens_cy) - (1.8 * s);
                let ico_cov = smoothstep(0.65 - d_ring.min(d_stem).min(d_dot));
                if ico_cov > 0.0 {
                    pr = pr * (1.0 - ico_cov) + accent[0] * ico_cov;
                    pg = pg * (1.0 - ico_cov) + accent[1] * ico_cov;
                    pb = pb * (1.0 - ico_cov) + accent[2] * ico_cov;
                    pa = pa.max(ico_cov);
                }

                let r8 = (pr.clamp(0.0, 1.0) * 255.0) as u32;
                let g8 = (pg.clamp(0.0, 1.0) * 255.0) as u32;
                let b8 = (pb.clamp(0.0, 1.0) * 255.0) as u32;
                let a8 = (pa.clamp(0.0, 1.0) * 255.0) as u32;
                buf[(py_i * ww + px_i) as usize] = (a8 << 24) | (r8 << 16) | (g8 << 8) | b8;
            }
        }

        let dst = POINT { x, y };
        let src = POINT { x: 0, y: 0 };
        let sz = SIZE { cx: ww, cy: wh };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,
        };
        UpdateLayeredWindow(
            hwnd,
            screen,
            &dst,
            &sz,
            mem,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        );

        SelectObject(mem, old);
        DeleteObject(dib as *mut _);
    }

    DeleteDC(mem);
    ReleaseDC(std::ptr::null_mut(), screen);
}

fn spawn_pill_thread() {
    thread::spawn(|| unsafe {
        let class_name: Vec<u16> = "NexusCircleTriggerPill\0".encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_HAND),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);

        let screen = GetDC(std::ptr::null_mut());
        let dpi = GetDeviceCaps(screen, LOGPIXELSX as i32);
        ReleaseDC(std::ptr::null_mut(), screen);
        let s: f32 = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };

        let ww = (WIN_W * s).round() as i32;
        let wh = (WIN_H * s).round() as i32;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            ww,
            wh,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }

        let should_show = {
            let mut rt = runtime().lock().unwrap();
            if !rt.desired_enabled {
                DestroyWindow(hwnd);
                return;
            }
            rt.hwnd = hwnd as isize;
            rt.scale = s;
            rt.hovered = false;
            !rt.temp_hidden
        };

        render_handle(hwnd, false, s);
        if should_show {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let mut rt = runtime().lock().unwrap();
        rt.hwnd = 0;
        rt.hovered = false;
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_VISIBILITY => {
            if wparam != 0 {
                let s = runtime().lock().unwrap().scale;
                render_handle(hwnd, false, s);
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            } else {
                ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            let s = runtime().lock().unwrap().scale;
            render_handle(hwnd, false, s);
            0
        }
        WM_MOUSEMOVE => {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND));
            let need_render = {
                let mut rt = runtime().lock().unwrap();
                if !rt.hovered {
                    rt.hovered = true;
                    Some(rt.scale)
                } else {
                    None
                }
            };
            if let Some(s) = need_render {
                render_handle(hwnd, true, s);
                SetTimer(hwnd, HOVER_TIMER_ID, 80, None);
            }
            0
        }
        WM_TIMER => {
            if wparam == HOVER_TIMER_ID {
                let mut pt = POINT { x: 0, y: 0 };
                let mut rc: RECT = std::mem::zeroed();
                if GetCursorPos(&mut pt) != 0 && GetWindowRect(hwnd, &mut rc) != 0 {
                    let inside =
                        pt.x >= rc.left && pt.x <= rc.right && pt.y >= rc.top && pt.y <= rc.bottom;
                    if !inside {
                        KillTimer(hwnd, HOVER_TIMER_ID);
                        let s = {
                            let mut rt = runtime().lock().unwrap();
                            rt.hovered = false;
                            rt.scale
                        };
                        render_handle(hwnd, false, s);
                    }
                }
            }
            0
        }
        WM_LBUTTONUP => {
            let cb = runtime().lock().unwrap().on_trigger.clone();
            if let Some(f) = cb {
                thread::spawn(move || f());
            }
            0
        }
        WM_CLOSE => {
            KillTimer(hwnd, HOVER_TIMER_ID);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_pill_sits_on_lower_right_clear_of_taskbar() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1032,
        };
        let (x, y) = compute_pill_origin(work, 1920, 1080, 64, 64, 1.0);
        // Right side near monitor right edge
        assert!(x >= 1840 && x <= 1900, "unexpected x={x}");
        // Lower-right ~80% down, clearly above taskbar
        assert!(y >= 800 && y + 64 < 1032, "unexpected y={y}");
    }
}
