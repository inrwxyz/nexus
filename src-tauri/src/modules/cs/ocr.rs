// cs/ocr.rs — Offline Windows.Media.Ocr recognition with line bounding boxes & surface color sampling.

use serde::Serialize;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

#[derive(Debug, Clone, Serialize)]
pub struct OcrLineBox {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub bg_color: String,
    pub fg_color: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrOutput {
    pub text: String,
    pub lines: Vec<String>,
    pub line_boxes: Vec<OcrLineBox>,
    pub line_count: usize,
}

struct ComGuard(bool);

impl ComGuard {
    fn init() -> Self {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            Self(hr.is_ok())
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Upscales captures and adds an edge-replicated quiet border so Windows.Media.Ocr detects tight crops
/// and small UI words sharply. Returns (scale_factor, pad_px, out_w, out_h, bgra_bytes).
fn prepare_ocr_buffer(w: u32, h: u32, bgra: &[u8]) -> (u32, u32, u32, u32, Vec<u8>) {
    let scale = if w < 90 || h < 36 {
        3
    } else if w * 2 <= 4096 && h * 2 <= 4096 {
        2
    } else {
        1
    };

    let s_w = w.saturating_mul(scale);
    let s_h = h.saturating_mul(scale);
    let pad = if w > 0 && h > 0 && s_w + 16 <= 4096 && s_h + 16 <= 4096 {
        8
    } else {
        0
    };

    if scale == 1 && pad == 0 {
        return (1, 0, w, h, bgra.to_vec());
    }

    let nw = s_w + pad * 2;
    let nh = s_h + pad * 2;
    let s = scale as usize;
    let wu = w as usize;
    let nwu = nw as usize;
    let pad_u = pad as usize;
    let mut out = vec![0u8; nwu * (nh as usize) * 4];

    for y in 0..(h as usize) {
        let src_row = &bgra[y * wu * 4..(y + 1) * wu * 4];
        let mut scaled_row = vec![0u8; s_w as usize * 4];
        for (x, px) in src_row.chunks_exact(4).enumerate() {
            for dx in 0..s {
                let base = (x * s + dx) * 4;
                scaled_row[base..base + 4].copy_from_slice(px);
            }
        }
        let first_px = &src_row[0..4];
        let last_px = &src_row[(wu - 1) * 4..wu * 4];
        for dy in 0..s {
            let row_idx = pad_u + y * s + dy;
            let dst_row = &mut out[row_idx * nwu * 4..(row_idx + 1) * nwu * 4];
            for px_i in 0..pad_u {
                dst_row[px_i * 4..(px_i + 1) * 4].copy_from_slice(first_px);
            }
            dst_row[pad_u * 4..(pad_u + s_w as usize) * 4].copy_from_slice(&scaled_row);
            for px_i in (pad_u + s_w as usize)..nwu {
                dst_row[px_i * 4..(px_i + 1) * 4].copy_from_slice(last_px);
            }
        }
    }

    if pad_u > 0 && !bgra.is_empty() {
        let top_row = out[pad_u * nwu * 4..(pad_u + 1) * nwu * 4].to_vec();
        let bot_idx = pad_u + s_h as usize - 1;
        let bot_row = out[bot_idx * nwu * 4..(bot_idx + 1) * nwu * 4].to_vec();
        for r in 0..pad_u {
            out[r * nwu * 4..(r + 1) * nwu * 4].copy_from_slice(&top_row);
        }
        for r in (pad_u + s_h as usize)..(nh as usize) {
            out[r * nwu * 4..(r + 1) * nwu * 4].copy_from_slice(&bot_row);
        }
    }

    (scale, pad, nw, nh, out)
}

/// Sample perimeter pixels just around/at a line's bounding rect to match surface background color & readable text contrast.
fn sample_box_colors(w: u32, h: u32, bgra: &[u8], lx: f32, ly: f32, lw: f32, lh: f32) -> (String, String) {
    if w == 0 || h == 0 || bgra.len() < (w as usize * h as usize * 4) {
        return ("rgb(15, 19, 25)".to_string(), "#f8fafc".to_string());
    }

    let x0 = (lx.floor() as i32 - 2).clamp(0, w as i32 - 1);
    let y0 = (ly.floor() as i32 - 2).clamp(0, h as i32 - 1);
    let x1 = ((lx + lw).ceil() as i32 + 2).clamp(0, w as i32 - 1);
    let y1 = ((ly + lh).ceil() as i32 + 2).clamp(0, h as i32 - 1);

    let mut rs = Vec::with_capacity(20);
    let mut gs = Vec::with_capacity(20);
    let mut bs = Vec::with_capacity(20);

    let mut push_px = |px: i32, py: i32| {
        let idx = (py as usize * w as usize + px as usize) * 4;
        if idx + 3 < bgra.len() {
            bs.push(bgra[idx]);
            gs.push(bgra[idx + 1]);
            rs.push(bgra[idx + 2]);
        }
    };

    let steps = 8i32;
    let span_x = (x1 - x0).max(1);
    for i in 0..=steps {
        let sx = x0 + (span_x * i) / steps;
        push_px(sx, y0);
        push_px(sx, y1);
    }
    let mid_y = (y0 + y1) / 2;
    push_px(x0, mid_y);
    push_px(x1, mid_y);

    if rs.is_empty() {
        return ("rgb(15, 19, 25)".to_string(), "#f8fafc".to_string());
    }

    rs.sort_unstable();
    gs.sort_unstable();
    bs.sort_unstable();
    let mid = rs.len() / 2;
    let r = rs[mid];
    let g = gs[mid];
    let b = bs[mid];

    let luma = 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32);
    if luma > 138.0 {
        (format!("rgb({r}, {g}, {b})"), "#0f172a".to_string())
    } else {
        (format!("rgb({r}, {g}, {b})"), "#f8fafc".to_string())
    }
}

pub fn extract_text_offline(
    w: u32,
    h: u32,
    bgra: &[u8],
    origin_x: u32,
    origin_y: u32,
) -> Result<OcrOutput, String> {
    if w == 0 || h == 0 || bgra.len() < (w as usize * h as usize * 4) {
        return Err("Empty or invalid image buffer for OCR".to_string());
    }
    let _com = ComGuard::init();

    let (scale, pad, ow, oh, buf) = prepare_ocr_buffer(w, h, bgra);
    let scale_f = scale as f32;
    let pad_f = pad as f32;
    let ibuf = CryptographicBuffer::CreateFromByteArray(&buf)
        .map_err(|e| format!("Failed to prepare OCR buffer: {e}"))?;

    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &ibuf,
        BitmapPixelFormat::Bgra8,
        ow as i32,
        oh as i32,
    )
    .map_err(|e| format!("Failed to build SoftwareBitmap: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .or_else(|_| {
            let lang = Language::CreateLanguage(&HSTRING::from("en-US"))?;
            OcrEngine::TryCreateFromLanguage(&lang)
        })
        .map_err(|e| {
            format!("Windows OCR engine unavailable (check Windows Language OCR pack): {e}")
        })?;

    let op = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("OCR recognition start failed: {e}"))?;
    let res = op
        .get()
        .map_err(|e| format!("OCR recognition failed: {e}"))?;

    let mut lines = Vec::new();
    let mut line_boxes = Vec::new();
    if let Ok(ocr_lines) = res.Lines() {
        let count = ocr_lines.Size().unwrap_or(0);
        for i in 0..count {
            if let Ok(line) = ocr_lines.GetAt(i) {
                let s = line
                    .Text()
                    .map(|t| t.to_string_lossy().trim().to_string())
                    .unwrap_or_default();
                if s.is_empty() {
                    continue;
                }

                let mut min_x = f32::INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut max_y = f32::NEG_INFINITY;

                if let Ok(words) = line.Words() {
                    let wc = words.Size().unwrap_or(0);
                    for wi in 0..wc {
                        if let Ok(word) = words.GetAt(wi) {
                            if let Ok(rect) = word.BoundingRect() {
                                min_x = min_x.min(rect.X);
                                min_y = min_y.min(rect.Y);
                                max_x = max_x.max(rect.X + rect.Width);
                                max_y = max_y.max(rect.Y + rect.Height);
                            }
                        }
                    }
                }

                let (lx, ly, lw, lh) = if min_x.is_finite() && max_x > min_x && max_y > min_y {
                    let x0 = ((min_x - pad_f) / scale_f).clamp(0.0, w as f32);
                    let y0 = ((min_y - pad_f) / scale_f).clamp(0.0, h as f32);
                    let x1 = ((max_x - pad_f) / scale_f).clamp(x0 + 1.0, w as f32);
                    let y1 = ((max_y - pad_f) / scale_f).clamp(y0 + 1.0, h as f32);
                    (x0, y0, x1 - x0, y1 - y0)
                } else {
                    (0.0, 0.0, w as f32, 20.0)
                };

                let (bg_color, fg_color) = sample_box_colors(w, h, bgra, lx, ly, lw, lh);
                lines.push(s.clone());
                line_boxes.push(OcrLineBox {
                    text: s,
                    x: origin_x as f32 + lx,
                    y: origin_y as f32 + ly,
                    width: lw,
                    height: lh,
                    bg_color,
                    fg_color,
                });
            }
        }
    }

    let text = if !lines.is_empty() {
        lines.join("\n")
    } else {
        res.Text()
            .map(|t| t.to_string_lossy().trim().to_string())
            .unwrap_or_default()
    };

    let line_count = lines.len();
    Ok(OcrOutput {
        text,
        lines,
        line_boxes,
        line_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_color_sampling_chooses_contrast_fg() {
        // 10x10 pure white BGRA -> dark foreground text
        let white = vec![255u8; 10 * 10 * 4];
        let (bg_w, fg_w) = sample_box_colors(10, 10, &white, 2.0, 2.0, 6.0, 5.0);
        assert_eq!(bg_w, "rgb(255, 255, 255)");
        assert_eq!(fg_w, "#0f172a");

        // 10x10 dark navy BGRA (B=30, G=20, R=15) -> light foreground text
        let mut dark = Vec::with_capacity(10 * 10 * 4);
        for _ in 0..100 {
            dark.extend_from_slice(&[30, 20, 15, 255]);
        }
        let (bg_d, fg_d) = sample_box_colors(10, 10, &dark, 2.0, 2.0, 6.0, 5.0);
        assert_eq!(bg_d, "rgb(15, 20, 30)");
        assert_eq!(fg_d, "#f8fafc");
    }

    #[test]
    fn padded_ocr_preserves_bounds_and_reads_tight_crop() {
        let w = 180u32;
        let h = 24u32;
        let (scale, pad, ow, oh, buf) = prepare_ocr_buffer(w, h, &vec![240u8; (w * h * 4) as usize]);
        assert_eq!(scale, 3);
        assert_eq!(pad, 8);
        assert_eq!(ow, w * 3 + 16);
        assert_eq!(oh, h * 3 + 16);
        assert_eq!(buf.len(), (ow * oh * 4) as usize);

        let bgra: Vec<u8> = unsafe {
            use windows_sys::Win32::Graphics::Gdi::{
                CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
                GdiFlush, SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO,
                BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, TRANSPARENT,
            };
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w as i32;
            bmi.bmiHeader.biHeight = -(h as i32);
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let old = SelectObject(dc, dib as *mut _);
            let slice = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            slice.fill(250);
            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let font = CreateFontW(-16, 0, 0, 0, 500, 0, 0, 0, 1, 0, 0, 4, 0, face.as_ptr());
            let old_f = SelectObject(dc, font as *mut _);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00111111);
            let txt: Vec<u16> = "Translate hello".encode_utf16().collect();
            TextOutW(dc, 2, 2, txt.as_ptr(), txt.len() as i32);
            GdiFlush();
            let out = slice.to_vec();
            SelectObject(dc, old_f);
            DeleteObject(font as *mut _);
            SelectObject(dc, old);
            DeleteObject(dib as *mut _);
            DeleteDC(dc);
            out
        };

        let out = extract_text_offline(w, h, &bgra, 100, 50).expect("OCR should succeed");
        assert!(out.text.contains("Translate"), "unexpected OCR: {}", out.text);
        let b = &out.line_boxes[0];
        assert!(b.x >= 100.0 && b.x < 100.0 + w as f32);
        assert!(b.y >= 50.0 && b.y < 50.0 + h as f32);
    }
}
