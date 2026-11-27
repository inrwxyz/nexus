//! Windows Win32 per-pixel-alpha overlay host for the Corner Assistant Pill.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::renderer::{
    ease_out_back, label_for, render_frame, smoothstep, Canvas, TextBitmap, FONT_PX, MARGIN, PH,
    PW, WIN_H, WIN_W,
};
use super::{PillShared, PillState};

#[cfg(target_os = "windows")]
pub fn spawn_pill_window(shared: Arc<Mutex<PillShared>>) {
    thread::spawn(move || unsafe { pill_thread(shared) });
}

/// Rasterize a string with GDI grayscale anti-aliasing and return its coverage map.
#[cfg(target_os = "windows")]
unsafe fn rasterize_text(text: &str, px_h: i32) -> TextBitmap {
    use windows_sys::Win32::Foundation::SIZE;
    use windows_sys::Win32::Graphics::Gdi::*;

    let screen = GetDC(std::ptr::null_mut());
    let dc = CreateCompatibleDC(screen);

    let font_name: Vec<u16> = "Segoe UI Variable\0".encode_utf16().collect();
    let font = CreateFontW(-px_h, 0, 0, 0, 600, 0, 0, 0, 1, 0, 0, 4, 0, font_name.as_ptr());
    let old_font = SelectObject(dc, font as *mut _);

    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut sz = SIZE { cx: 0, cy: 0 };
    GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut sz);
    let tw = (sz.cx + 6).max(1);
    let th = (sz.cy + 2).max(1);

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = tw;
    bmi.bmiHeader.biHeight = -th;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);

    let mut cov = vec![0u8; (tw * th) as usize];
    if !bmp.is_null() && !bits.is_null() {
        let old_bmp = SelectObject(dc, bmp as *mut _);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ffffff);
        TextOutW(dc, 3, 1, wide.as_ptr(), wide.len() as i32);
        GdiFlush();
        let px = std::slice::from_raw_parts(bits as *const u32, (tw * th) as usize);
        for (i, p) in px.iter().enumerate() {
            cov[i] = ((p >> 8) & 0xff) as u8;
        }
        SelectObject(dc, old_bmp);
        DeleteObject(bmp as *mut _);
    }

    SelectObject(dc, old_font);
    DeleteObject(font as *mut _);
    DeleteDC(dc);
    ReleaseDC(std::ptr::null_mut(), screen);

    TextBitmap { w: tw as usize, h: th as usize, cov }
}

/// Computes bottom-right coordinates keeping clear of the Windows 11 taskbar/system tray even
/// when SPI_GETWORKAREA matches full monitor bounds (e.g. taskbar auto-hide or overlay).
pub fn compute_pill_origin(
    work: windows_sys::Win32::Foundation::RECT,
    monitor_bottom: i32,
    s: f32,
) -> (i32, i32) {
    let edge = (22.0 * s).round() as i32;
    let x = work.right - edge - ((PW + MARGIN) * s).round() as i32;
    let effective_bottom = if work.bottom >= monitor_bottom - (8.0 * s).round() as i32 {
        monitor_bottom - (48.0 * s).round() as i32
    } else {
        work.bottom
    };
    let clearance = (24.0 * s).round() as i32;
    let y = effective_bottom - clearance - ((PH + MARGIN) * s).round() as i32;
    (x, y)
}

#[cfg(target_os = "windows")]
unsafe fn pill_thread(shared: Arc<Mutex<PillShared>>) {
    use windows_sys::Win32::Foundation::{POINT, RECT, SIZE};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let class_name: Vec<u16> = "NexusPill\0".encode_utf16().collect();

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: std::ptr::null_mut(),
        hIcon: std::ptr::null_mut(),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    RegisterClassExW(&wc);

    let screen = GetDC(std::ptr::null_mut());
    let dpi = GetDeviceCaps(screen, LOGPIXELSX as i32);
    let s: f32 = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    let ww = (WIN_W * s).round() as i32;
    let wh = (WIN_H * s).round() as i32;

    let place = move || -> (i32, i32) {
        let mut work: RECT = std::mem::zeroed();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut std::ffi::c_void,
            0,
        );
        let mon_h = GetSystemMetrics(SM_CYSCREEN);
        let mon_bottom = if mon_h > 0 { mon_h } else { work.bottom };
        compute_pill_origin(work, mon_bottom, s)
    };
    let (x, y) = place();

    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
        class_name.as_ptr(),
        std::ptr::null(),
        WS_POPUP,
        x,
        y,
        ww,
        wh,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null(),
    );
    if hwnd.is_null() {
        ReleaseDC(std::ptr::null_mut(), screen);
        return;
    }

    let mem_dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = ww;
    bmi.bmiHeader.biHeight = -wh;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
    ReleaseDC(std::ptr::null_mut(), screen);
    if dib.is_null() || bits.is_null() {
        DeleteDC(mem_dc);
        DestroyWindow(hwnd);
        return;
    }
    let old_dib = SelectObject(mem_dc, dib as *mut _);
    let frame: &mut [u32] = std::slice::from_raw_parts_mut(bits as *mut u32, (ww * wh) as usize);

    let mut canvas = Canvas::new(ww as usize, wh as usize, s);
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let size = SIZE { cx: ww, cy: wh };
    let src_pt = POINT { x: 0, y: 0 };

    let mut shown = PillState::Hidden;
    let mut text_bmp: Option<TextBitmap> = None;
    let mut text_alpha: f32 = 1.0;
    let mut vis_lin: f32 = 0.0;
    let mut age: f32 = 0.0;
    let mut t: f32 = 0.0;
    let mut visible = false;
    let mut last = Instant::now();

    loop {
        let mut msg = std::mem::zeroed::<MSG>();
        while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.05);
        last = now;

        let cur = {
            let mut sh = shared.lock().unwrap();
            sh.dirty = false;
            sh.state.clone()
        };
        if matches!(cur, PillState::Quit) {
            DestroyWindow(hwnd);
            SelectObject(mem_dc, old_dib);
            DeleteObject(dib as *mut _);
            DeleteDC(mem_dc);
            break;
        }
        let active = !matches!(cur, PillState::Hidden);

        if active && cur != shown {
            shown = cur.clone();
            age = 0.0;
            let (label, a) = label_for(&shown);
            text_alpha = a;
            text_bmp = if label.is_empty() {
                None
            } else {
                Some(rasterize_text(&label, (FONT_PX as f32 * s).round() as i32))
            };
        }

        if active {
            vis_lin = (vis_lin + dt / 0.50).min(1.0);
        } else {
            vis_lin = (vis_lin - dt / 0.28).max(0.0);
        }

        if !active && vis_lin <= 0.0 {
            if visible {
                ShowWindow(hwnd, SW_HIDE);
                visible = false;
            }
            thread::sleep(Duration::from_millis(30));
            continue;
        }

        t += dt;
        age += dt;

        render_frame(
            &mut canvas,
            &shown,
            smoothstep((vis_lin * 2.5).min(1.0)),
            if active { ease_out_back(vis_lin) } else { smoothstep(vis_lin) },
            age,
            t,
            text_bmp.as_ref(),
            text_alpha,
        );

        for (dst, p) in frame.iter_mut().zip(canvas.px.iter()) {
            let a = (p[3] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let r = (p[0] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let g = (p[1] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let b = (p[2] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            *dst = (a << 24) | (r << 16) | (g << 8) | b;
        }

        if !visible {
            let (nx, ny) = place();
            SetWindowPos(hwnd, HWND_TOPMOST, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }

        UpdateLayeredWindow(
            hwnd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
            mem_dc,
            &src_pt,
            0,
            &blend,
            ULW_ALPHA,
        );

        if !visible {
            ShowWindow(hwnd, SW_SHOWNA);
            visible = true;
        }

        thread::sleep(Duration::from_millis(16));
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wp: windows_sys::Win32::Foundation::WPARAM,
    lp: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    match msg {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_DESTROY => {
            windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            0
        }
        _ => windows_sys::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wp, lp),
    }
}
