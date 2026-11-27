// screenshot.rs — "take screenshot" / "snip screenshot" voice + phone commands.
//
//   take screenshot  → grabs the monitor under the mouse and saves a PNG straight into the
//                      Windows Screenshots folder (Pictures\Screenshots), no clicks needed.
//   snip screenshot  → opens Windows' own crop overlay (ms-screenclip:) so you pick the area.
//
// Fully native: GDI BitBlt capture + the `png` crate + SHGetKnownFolderPath + ShellExecuteW.
// No helper processes, nothing stays resident.
//
//   screenshot::handle_voice_command(lnorm) -> Option<bool>

use std::ffi::c_void;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{POINT, RECT, SYSTEMTIME};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
    GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SW_SHOWNORMAL};

/// Captures the monitor under the cursor as top-down BGRA.
fn capture_monitor() -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut mi) == 0 {
            return Err("monitor bounds unavailable".into());
        }
        let rect: RECT = mi.rcMonitor;
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;

        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return Err("screen DC unavailable".into());
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return Err("CreateCompatibleDC failed".into());
        }

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return Err("CreateDIBSection failed".into());
        }

        let old = SelectObject(mem_dc, dib as *mut _);
        let ok = BitBlt(
            mem_dc, 0, 0, width as i32, height as i32, screen_dc, rect.left, rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        GdiFlush();

        let len = width as usize * height as usize * 4;
        let mut bgra = Vec::new();
        if ok != 0 {
            bgra.extend_from_slice(std::slice::from_raw_parts(bits as *const u8, len));
        }

        SelectObject(mem_dc, old);
        DeleteObject(dib as *mut _);
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if ok == 0 {
            return Err("BitBlt failed".into());
        }
        Ok((width, height, bgra))
    }
}

fn encode_png(w: u32, h: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Pictures\Screenshots (the folder Win+PrtScn uses), created if missing.
fn screenshots_dir() -> Option<PathBuf> {
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut p) != 0 || p.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        let pictures = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p as *const c_void);
        let dir = PathBuf::from(pictures).join("Screenshots");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }
}

fn file_stamp() -> String {
    unsafe {
        let mut t: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut t);
        format!(
            "Screenshot {:04}-{:02}-{:02} {:02}{:02}{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
}

/// Full-screen shot of the monitor under the mouse → PNG in Pictures\Screenshots.
pub fn take() -> bool {
    let run = || -> Result<PathBuf, String> {
        let dir = screenshots_dir().ok_or("Screenshots folder unavailable")?;
        let (w, h, bgra) = capture_monitor()?;
        let png = encode_png(w, h, &bgra)?;
        let path = dir.join(format!("{}.png", file_stamp()));
        std::fs::write(&path, png).map_err(|e| e.to_string())?;
        Ok(path)
    };
    match run() {
        Ok(path) => {
            println!("📸 Screenshot saved: {}", path.display());
            crate::sound::play_screenshot_sound();
            true
        }
        Err(e) => {
            eprintln!("Screenshot failed: {e}");
            false
        }
    }
}

/// Windows' own crop overlay: the user drags the area, Windows handles saving / clipboard.
pub fn snip() -> bool {
    let target: Vec<u16> = "ms-screenclip:".encode_utf16().chain(std::iter::once(0)).collect();
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        (r as isize) > 32
    }
}

// ───────────────────────── voice parsing ─────────────────────────

/// `l` is the already-normalised command line (lower-case, punctuation → spaces).
/// Some(result) when the line is a screenshot command, None otherwise.
pub fn handle_voice_command(l: &str) -> Option<bool> {
    // speech engines split / misspell the word a lot
    let l = l
        .replace("screen shot", "screenshot")
        .replace("screenshots", "screenshot")
        .replace("screen shots", "screenshot")
        .replace("screen capture", "screenshot")
        .replace("capture screen", "screenshot")
        .replace("screen grab", "screenshot")
        .replace("scrinshot", "screenshot")
        .replace("skrinshot", "screenshot")
        .replace("scrnshot", "screenshot");
    if !l.contains("screenshot") {
        return None;
    }
    let words: Vec<&str> = l.split_whitespace().collect();
    let has = |w: &str| words.contains(&w);

    // "set screenshot" (how it is usually dictated) and snip / crop / select → pick the area
    if has("set") || has("snip") || has("crop") || has("select") || has("area") || has("selected") {
        println!("✂️ Screenshot: crop overlay");
        return Some(snip());
    }
    // "take screenshot", "take a screenshot", plain "screenshot", "screenshot nao/tolo"
    println!("📸 Screenshot: full screen");
    Some(take())
}
