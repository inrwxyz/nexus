// cs/capture.rs — Fast Win32 active-monitor screen capture, in-memory BMP/PNG encoding, crop & clipboard.

use std::ffi::c_void;
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
    GetDeviceCaps, GetMonitorInfoW, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, LOGPIXELSX, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub struct FrozenFrame {
    pub width: u32,
    pub height: u32,
    pub monitor_left: i32,
    pub monitor_top: i32,
    pub scale: f64,
    pub bgra: Vec<u8>,
}

impl FrozenFrame {
    /// Logical monitor bounds (x, y, width, height) matching Tauri window coordinates.
    pub fn logical_rect(&self) -> (f64, f64, f64, f64) {
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        (
            self.monitor_left as f64 / s,
            self.monitor_top as f64 / s,
            self.width as f64 / s,
            self.height as f64 / s,
        )
    }

    /// Encode top-down 32-bit BGRA pixels into an uncompressed BMP byte buffer (<2ms, zero PNG CPU).
    pub fn to_bmp(&self) -> Vec<u8> {
        let pixel_bytes = self.bgra.len();
        let header_size = 14 + 40;
        let total_size = header_size + pixel_bytes;
        let mut out = Vec::with_capacity(total_size);

        // BITMAPFILEHEADER (14 bytes)
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(total_size as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(header_size as u32).to_le_bytes());

        // BITMAPINFOHEADER (40 bytes), negative height = top-down scanlines matching BitBlt DIB
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(self.width as i32).to_le_bytes());
        out.extend_from_slice(&(-(self.height as i32)).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        out.extend_from_slice(&self.bgra);
        out
    }

    /// Safely crop a rectangle from the frozen frame as BGRA pixels with opaque alpha.
    pub fn crop_bgra(&self, x: u32, y: u32, width: u32, height: u32) -> Result<(u32, u32, Vec<u8>), String> {
        if self.width == 0 || self.height == 0 {
            return Err("Frozen frame is empty".to_string());
        }
        let x0 = x.min(self.width.saturating_sub(1));
        let y0 = y.min(self.height.saturating_sub(1));
        let w = width.min(self.width - x0);
        let h = height.min(self.height - y0);
        if w < 4 || h < 4 {
            return Err("Selection is too small".to_string());
        }

        let stride = self.width as usize * 4;
        let row_bytes = w as usize * 4;
        let mut out = vec![0u8; row_bytes * h as usize];

        for row in 0..h as usize {
            let src_start = (y0 as usize + row) * stride + x0 as usize * 4;
            let dst_start = row * row_bytes;
            let src = &self.bgra[src_start..src_start + row_bytes];
            let dst = &mut out[dst_start..dst_start + row_bytes];
            dst.copy_from_slice(src);
            for px in dst.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        Ok((w, h, out))
    }
}

/// Captures the monitor currently under the cursor into an in-memory 32-bit BGRA buffer.
pub fn capture_active_monitor() -> Result<FrozenFrame, String> {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);

        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut mi) == 0 {
            return Err("Failed to query active monitor bounds".to_string());
        }
        let rect: RECT = mi.rcMonitor;
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;

        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return Err("Failed to acquire screen DC".to_string());
        }

        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) != 0 || dpi_x == 0 {
            let sys_dpi = GetDeviceCaps(screen_dc, LOGPIXELSX as i32);
            dpi_x = if sys_dpi > 0 { sys_dpi as u32 } else { 96 };
        }
        let scale = dpi_x as f64 / 96.0;

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return Err("CreateCompatibleDC failed".to_string());
        }

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width as i32;
        bmi.bmiHeader.biHeight = -(height as i32); // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(
            screen_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if dib.is_null() || bits.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return Err("CreateDIBSection failed".to_string());
        }

        let old_obj = SelectObject(mem_dc, dib as *mut _);
        let ok = BitBlt(
            mem_dc,
            0,
            0,
            width as i32,
            height as i32,
            screen_dc,
            rect.left,
            rect.top,
            SRCCOPY | CAPTUREBLT,
        );
        GdiFlush();

        let byte_len = width as usize * height as usize * 4;
        let mut bgra = Vec::new();
        if ok != 0 {
            let src = std::slice::from_raw_parts(bits as *const u8, byte_len);
            bgra.resize(byte_len, 0);
            bgra.copy_from_slice(src);
        }

        SelectObject(mem_dc, old_obj);
        DeleteObject(dib as *mut _);
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if ok == 0 {
            return Err("BitBlt screen capture failed".to_string());
        }

        Ok(FrozenFrame {
            width,
            height,
            monitor_left: rect.left,
            monitor_top: rect.top,
            scale,
            bgra,
        })
    }
}

/// Encode opaque RGBA PNG bytes from top-down BGRA pixels.
pub fn encode_png(w: u32, h: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.push(px[2]); // R
        rgba.push(px[1]); // G
        rgba.push(px[0]); // B
        rgba.push(255);   // A
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Put a cropped BGRA image onto the Windows Clipboard as CF_DIB (for instant Ctrl+V paste/Lens fallback).
pub fn copy_bgra_to_clipboard(w: u32, h: u32, bgra: &[u8]) -> bool {
    let hdr_size = std::mem::size_of::<BITMAPINFOHEADER>();
    let total = hdr_size + bgra.len();

    let mut hdr: BITMAPINFOHEADER = unsafe { std::mem::zeroed() };
    hdr.biSize = hdr_size as u32;
    hdr.biWidth = w as i32;
    hdr.biHeight = -(h as i32); // top-down DIB
    hdr.biPlanes = 1;
    hdr.biBitCount = 32;
    hdr.biCompression = BI_RGB;
    hdr.biSizeImage = bgra.len() as u32;

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h_mem = GlobalAlloc(0x0002, total); // GMEM_MOVEABLE
        if !h_mem.is_null() {
            let ptr = GlobalLock(h_mem) as *mut u8;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(&hdr as *const _ as *const u8, ptr, hdr_size);
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), ptr.add(hdr_size), bgra.len());
                GlobalUnlock(h_mem);
                // CF_DIB = 8
                SetClipboardData(8, h_mem);
            }
        }
        CloseClipboard();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_bmp_and_png_encoding_work() {
        let frame = FrozenFrame {
            width: 16,
            height: 12,
            monitor_left: 0,
            monitor_top: 0,
            scale: 1.0,
            bgra: vec![0x80; 16 * 12 * 4],
        };
        let bmp = frame.to_bmp();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(bmp.len(), 54 + 16 * 12 * 4);

        let (cw, ch, cropped) = frame.crop_bgra(10, 8, 20, 20).unwrap();
        assert_eq!((cw, ch), (6, 4));
        assert_eq!(cropped.len(), 6 * 4 * 4);
        assert_eq!(cropped[3], 255);

        let png = encode_png(cw, ch, &cropped).unwrap();
        assert_eq!(&png[0..4], b"\x89PNG");
    }
}
