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
    const MAX_OCR_DIM: u32 = 4400;
    let scale = if (w < 120 || h < 44) && w * 3 <= MAX_OCR_DIM && h * 3 <= MAX_OCR_DIM {
        3
    } else if w * 2 <= MAX_OCR_DIM && h * 2 <= MAX_OCR_DIM {
        2
    } else {
        1
    };

    let s_w = w.saturating_mul(scale);
    let s_h = h.saturating_mul(scale);
    let pad = if w > 0 && h > 0 && s_w + 16 <= MAX_OCR_DIM && s_h + 16 <= MAX_OCR_DIM {
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

    let x0 = (lx.floor() as i32 - 4).clamp(0, w as i32 - 1);
    let y0 = (ly.floor() as i32 - 4).clamp(0, h as i32 - 1);
    let x1 = ((lx + lw).ceil() as i32 + 4).clamp(0, w as i32 - 1);
    let y1 = ((ly + lh).ceil() as i32 + 4).clamp(0, h as i32 - 1);

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

fn sample_luma_stats(bgra: &[u8]) -> (usize, usize, usize) {
    let px_count = bgra.len() / 4;
    if px_count == 0 {
        return (0, 0, 0);
    }
    let step = (px_count / 2500).max(1);
    let mut dark = 0usize;
    let mut bright = 0usize;
    let mut total = 0usize;
    for px in bgra.chunks_exact(4).step_by(step) {
        let luma = (77u32 * px[2] as u32 + 150u32 * px[1] as u32 + 29u32 * px[0] as u32) >> 8;
        if luma < 110 {
            dark += 1;
        }
        if luma >= 215 {
            bright += 1;
        }
        total += 1;
    }
    (dark, bright, total)
}

/// Smoothly inverts dark UI regions (preserving ClearType antialiasing across 30..210 luma)
/// so muted grey-on-dark UI text reads cleanly when Pass 1 misses it.
fn build_smooth_dark_invert(bgra: &[u8]) -> Option<Vec<u8>> {
    let (dark, _, total) = sample_luma_stats(bgra);
    if total == 0 || dark * 100 < total * 8 {
        return None;
    }

    let mut out = vec![255u8; bgra.len()];
    for (src, dst) in bgra.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let luma = (77u32 * src[2] as u32 + 150u32 * src[1] as u32 + 29u32 * src[0] as u32) >> 8;
        let v = if luma <= 30 {
            255u8
        } else if luma >= 210 {
            0u8
        } else {
            (255 - ((luma - 30) * 255 / 180)) as u8
        };
        dst[0] = v;
        dst[1] = v;
        dst[2] = v;
        dst[3] = 255;
    }
    Some(out)
}

/// Smoothly extracts bright white glyphs on colored/pastel surfaces (such as desktop icon labels
/// over wallpaper) into dark-on-white without harsh binary clipping.
fn build_smooth_bright_mask(bgra: &[u8]) -> Option<Vec<u8>> {
    let (_, bright, total) = sample_luma_stats(bgra);
    if total == 0 || bright * 1000 < total || bright * 100 > total * 48 {
        return None;
    }

    let mut out = vec![255u8; bgra.len()];
    for (src, dst) in bgra.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        let luma = (77u32 * src[2] as u32 + 150u32 * src[1] as u32 + 29u32 * src[0] as u32) >> 8;
        let max_c = src[0].max(src[1]).max(src[2]);
        let min_c = src[0].min(src[1]).min(src[2]);
        let chroma = max_c - min_c;
        let v = if chroma > 54 || luma <= 178 {
            255u8
        } else if luma >= 232 {
            0u8
        } else {
            ((232 - luma) * 255 / 54) as u8
        };
        dst[0] = v;
        dst[1] = v;
        dst[2] = v;
        dst[3] = 255;
    }
    Some(out)
}

fn boxes_overlap(a: &OcrLineBox, b: &OcrLineBox) -> bool {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;

    let ix = (ax2.min(bx2) - a.x.max(b.x)).max(0.0);
    let iy = (ay2.min(by2) - a.y.max(b.y)).max(0.0);
    if ix <= 0.0 || iy <= 0.0 {
        return false;
    }

    let inter = ix * iy;
    let area_a = (a.width * a.height).max(1.0);
    let area_b = (b.width * b.height).max(1.0);
    inter > area_a.min(area_b) * 0.28
}

fn collect_ocr_engines() -> Result<Vec<OcrEngine>, String> {
    let mut engines = Vec::new();
    let mut seen_tags: Vec<String> = Vec::new();

    if let Ok(primary) = OcrEngine::TryCreateFromUserProfileLanguages().or_else(|_| {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US"))?;
        OcrEngine::TryCreateFromLanguage(&lang)
    }) {
        let tag = primary
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !tag.is_empty() {
            seen_tags.push(tag);
        }
        engines.push(primary);
    }

    // Also probe distinct non-English script packs installed on Windows (e.g. hi-IN, ja-JP, zh-CN, ar-SA)
    if let Ok(langs) = OcrEngine::AvailableRecognizerLanguages() {
        let count = langs.Size().unwrap_or(0);
        for i in 0..count {
            if engines.len() >= 4 {
                break;
            }
            if let Ok(lang) = langs.GetAt(i) {
                let tag = lang
                    .LanguageTag()
                    .map(|t| t.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let primary_sub = tag.split('-').next().unwrap_or("");
                let already_same_family = seen_tags
                    .iter()
                    .any(|s| s == &tag || s.split('-').next().unwrap_or("") == primary_sub);
                if !already_same_family {
                    if let Ok(eng) = OcrEngine::TryCreateFromLanguage(&lang) {
                        seen_tags.push(tag);
                        engines.push(eng);
                    }
                }
            }
        }
    }

    if engines.is_empty() {
        return Err(
            "Windows OCR engine unavailable (check Windows Language OCR pack)".to_string(),
        );
    }
    Ok(engines)
}

fn build_software_bitmap(ow: u32, oh: u32, buf: &[u8]) -> Result<SoftwareBitmap, String> {
    let ibuf = CryptographicBuffer::CreateFromByteArray(buf)
        .map_err(|e| format!("Failed to prepare OCR buffer: {e}"))?;
    SoftwareBitmap::CreateCopyFromBuffer(
        &ibuf,
        BitmapPixelFormat::Bgra8,
        ow as i32,
        oh as i32,
    )
    .map_err(|e| format!("Failed to build SoftwareBitmap: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn push_ocr_box(
    out: &mut Vec<OcrLineBox>,
    text: String,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    scale_f: f32,
    pad_f: f32,
    orig_w: u32,
    orig_h: u32,
    orig_bgra: &[u8],
    origin_x: u32,
    origin_y: u32,
) {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return;
    }

    let (lx, ly, lw, lh) = if min_x.is_finite() && max_x > min_x && max_y > min_y {
        let x0 = ((min_x - pad_f) / scale_f).clamp(0.0, orig_w as f32);
        let y0 = ((min_y - pad_f) / scale_f).clamp(0.0, orig_h as f32);
        let x1 = ((max_x - pad_f) / scale_f).clamp(x0 + 1.0, orig_w as f32);
        let y1 = ((max_y - pad_f) / scale_f).clamp(y0 + 1.0, orig_h as f32);
        (x0, y0, x1 - x0, y1 - y0)
    } else {
        (0.0, 0.0, orig_w as f32, 20.0)
    };

    let (bg_color, fg_color) = sample_box_colors(orig_w, orig_h, orig_bgra, lx, ly, lw, lh);
    out.push(OcrLineBox {
        text: trimmed,
        x: origin_x as f32 + lx,
        y: origin_y as f32 + ly,
        width: lw,
        height: lh,
        bg_color,
        fg_color,
    });
}

#[allow(clippy::too_many_arguments)]
fn run_ocr_with_engine(
    engine: &OcrEngine,
    bitmap: &SoftwareBitmap,
    scale_f: f32,
    pad_f: f32,
    orig_w: u32,
    orig_h: u32,
    orig_bgra: &[u8],
    origin_x: u32,
    origin_y: u32,
) -> Result<(Vec<OcrLineBox>, String), String> {
    let op = engine
        .RecognizeAsync(bitmap)
        .map_err(|e| format!("OCR recognition start failed: {e}"))?;
    let res = op
        .get()
        .map_err(|e| format!("OCR recognition failed: {e}"))?;

    let mut out = Vec::new();
    let fallback_text = res
        .Text()
        .map(|t| t.to_string_lossy().trim().to_string())
        .unwrap_or_default();

    let Ok(ocr_lines) = res.Lines() else {
        return Ok((out, fallback_text));
    };
    let count = ocr_lines.Size().unwrap_or(0);
    for i in 0..count {
        let Ok(line) = ocr_lines.GetAt(i) else {
            continue;
        };
        let line_text = line
            .Text()
            .map(|t| t.to_string_lossy().trim().to_string())
            .unwrap_or_default();
        if line_text.is_empty() {
            continue;
        }

        let mut word_items: Vec<(String, f32, f32, f32, f32)> = Vec::new();
        if let Ok(words) = line.Words() {
            let wc = words.Size().unwrap_or(0);
            for wi in 0..wc {
                if let Ok(word) = words.GetAt(wi) {
                    let wt = word
                        .Text()
                        .map(|t| t.to_string_lossy().trim().to_string())
                        .unwrap_or_default();
                    if let Ok(rect) = word.BoundingRect() {
                        if !wt.is_empty() && rect.Width > 0.0 && rect.Height > 0.0 {
                            word_items.push((
                                wt,
                                rect.X,
                                rect.Y,
                                rect.X + rect.Width,
                                rect.Y + rect.Height,
                            ));
                        }
                    }
                }
            }
        }

        if word_items.is_empty() {
            push_ocr_box(
                &mut out,
                line_text,
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                scale_f,
                pad_f,
                orig_w,
                orig_h,
                orig_bgra,
                origin_x,
                origin_y,
            );
            continue;
        }

        word_items.sort_by(|a, b| a.1.total_cmp(&b.1));
        let has_wide_gap = word_items.windows(2).any(|pair| {
            let h_est = (pair[0].4 - pair[0].2).max(12.0 * scale_f);
            pair[1].1 > pair[0].3 + (h_est * 3.0).max(36.0 * scale_f)
        });

        // Fast exact path: keep verbatim line.Text() when words are contiguous (preserves punctuation spacing).
        if !has_wide_gap {
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for (_, x0, y0, x1, y1) in &word_items {
                min_x = min_x.min(*x0);
                min_y = min_y.min(*y0);
                max_x = max_x.max(*x1);
                max_y = max_y.max(*y1);
            }
            push_ocr_box(
                &mut out, line_text, min_x, min_y, max_x, max_y, scale_f, pad_f, orig_w, orig_h,
                orig_bgra, origin_x, origin_y,
            );
            continue;
        }

        let mut seg_words = Vec::new();
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for (wt, wx0, wy0, wx1, wy1) in word_items {
            if !seg_words.is_empty() {
                let h_est = (max_y - min_y).max(12.0 * scale_f);
                if wx0 > max_x + (h_est * 3.0).max(36.0 * scale_f) {
                    let seg_txt = seg_words.join(" ");
                    if seg_txt.chars().any(|c| c.is_alphanumeric()) {
                        push_ocr_box(
                            &mut out, seg_txt, min_x, min_y, max_x, max_y, scale_f, pad_f, orig_w,
                            orig_h, orig_bgra, origin_x, origin_y,
                        );
                    }
                    seg_words.clear();
                    min_x = f32::INFINITY;
                    min_y = f32::INFINITY;
                    max_x = f32::NEG_INFINITY;
                    max_y = f32::NEG_INFINITY;
                }
            }
            seg_words.push(wt);
            min_x = min_x.min(wx0);
            min_y = min_y.min(wy0);
            max_x = max_x.max(wx1);
            max_y = max_y.max(wy1);
        }
        let seg_txt = seg_words.join(" ");
        if seg_txt.chars().any(|c| c.is_alphanumeric()) {
            push_ocr_box(
                &mut out, seg_txt, min_x, min_y, max_x, max_y, scale_f, pad_f, orig_w, orig_h,
                orig_bgra, origin_x, origin_y,
            );
        }
    }
    Ok((out, fallback_text))
}

fn merge_best_boxes(dst: &mut Vec<OcrLineBox>, extra: Vec<OcrLineBox>) {
    for candidate in extra {
        if !dst.iter().any(|existing| boxes_overlap(existing, &candidate)) {
            dst.push(candidate);
        }
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
    let bitmap = build_software_bitmap(ow, oh, &buf)?;
    let engines = collect_ocr_engines()?;

    // Pass 1: Pure, unclipped ClearType OCR pass.
    let mut line_boxes = Vec::new();
    let mut fallback_res_text = String::new();
    for engine in &engines {
        let (found, raw_txt) =
            run_ocr_with_engine(engine, &bitmap, scale_f, pad_f, w, h, bgra, origin_x, origin_y)?;
        if fallback_res_text.is_empty() && !raw_txt.is_empty() {
            fallback_res_text = raw_txt;
        }
        merge_best_boxes(&mut line_boxes, found);
    }

    // Pass 2 & 3 (non-destructive smooth ramps): rescue muted grey dark-mode UI lines and
    // bright white labels on colored surfaces/wallpapers, adding ONLY non-overlapping missed boxes.
    for rescue_opt in [build_smooth_dark_invert(bgra), build_smooth_bright_mask(bgra)] {
        if let Some(rescue_src) = rescue_opt {
            let (_, _, row, roh, rbuf) = prepare_ocr_buffer(w, h, &rescue_src);
            if row == ow && roh == oh {
                if let Ok(rbitmap) = build_software_bitmap(row, roh, &rbuf) {
                    for engine in &engines {
                        if let Ok((found, raw_txt)) = run_ocr_with_engine(
                            engine, &rbitmap, scale_f, pad_f, w, h, bgra, origin_x, origin_y,
                        ) {
                            if fallback_res_text.is_empty() && !raw_txt.is_empty() {
                                fallback_res_text = raw_txt;
                            }
                            let clean: Vec<OcrLineBox> = found
                                .into_iter()
                                .filter(|b| b.text.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
                                .collect();
                            merge_best_boxes(&mut line_boxes, clean);
                        }
                    }
                }
            }
        }
    }

    line_boxes.sort_by(|a, b| {
        let row_diff = (a.y - b.y).abs();
        if row_diff < 10.0 {
            a.x.total_cmp(&b.x)
        } else {
            a.y.total_cmp(&b.y)
        }
    });

    let lines: Vec<String> = line_boxes.iter().map(|b| b.text.clone()).collect();
    let text = if !lines.is_empty() {
        lines.join("\n")
    } else {
        fallback_res_text
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

    fn render_test_bgra(
        w: u32,
        h: u32,
        bg_bgr: [u8; 3],
        fg_colorref: u32,
        font_h: i32,
        tx: i32,
        ty: i32,
        text: &str,
    ) -> Vec<u8> {
        unsafe {
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
            for px in slice.chunks_exact_mut(4) {
                px[0] = bg_bgr[0];
                px[1] = bg_bgr[1];
                px[2] = bg_bgr[2];
                px[3] = 255;
            }
            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let font = CreateFontW(font_h, 0, 0, 0, 500, 0, 0, 0, 1, 0, 0, 4, 0, face.as_ptr());
            let old_f = SelectObject(dc, font as *mut _);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, fg_colorref);
            let txt: Vec<u16> = text.encode_utf16().collect();
            TextOutW(dc, tx, ty, txt.as_ptr(), txt.len() as i32);
            GdiFlush();
            let out = slice.to_vec();
            SelectObject(dc, old_f);
            DeleteObject(font as *mut _);
            SelectObject(dc, old);
            DeleteObject(dib as *mut _);
            DeleteDC(dc);
            out
        }
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

        let bgra = render_test_bgra(w, h, [250, 250, 250], 0x00111111, -16, 2, 2, "Translate hello");
        let out = extract_text_offline(w, h, &bgra, 100, 50).expect("OCR should succeed");
        assert!(out.text.contains("Translate"), "unexpected OCR: {}", out.text);
        let b = &out.line_boxes[0];
        assert!(b.x >= 100.0 && b.x < 100.0 + w as f32);
        assert!(b.y >= 50.0 && b.y < 50.0 + h as f32);
    }

    #[test]
    fn dark_fallback_reads_muted_grey_dark_ui_text_cleanly() {
        let bgra = render_test_bgra(
            260,
            40,
            [27, 24, 24],
            0x0081766e,
            -15,
            16,
            10,
            "Settings Sidebar Menu",
        );
        assert!(build_smooth_dark_invert(&bgra).is_some());
        let out = extract_text_offline(260, 40, &bgra, 0, 0).expect("dark fallback OCR failed");
        assert!(
            out.text.contains("Settings") && out.text.contains("Menu"),
            "muted dark UI OCR missed text: {}",
            out.text
        );
    }

    #[test]
    fn smooth_rescue_reads_mixed_dark_and_bright_desktop_labels() {
        let w = 480u32;
        let h = 120u32;
        let mut bgra = render_test_bgra(
            w,
            h,
            [27, 24, 24],
            0x00988f8a, // Muted grey prompt bar (#8a8f98 -> BGR 0x00988f8a)
            -15,
            210,
            44,
            "Ask anything now",
        );
        let strip = render_test_bgra(180, h, [214, 165, 180], 0x00FFFFFF, -13, 18, 44, "Google Chrome");
        for y in 0..h as usize {
            let src_off = y * 180 * 4;
            let dst_off = y * w as usize * 4;
            bgra[dst_off..dst_off + 180 * 4].copy_from_slice(&strip[src_off..src_off + 180 * 4]);
        }
        assert!(build_smooth_bright_mask(&bgra).is_some());
        let out = extract_text_offline(w, h, &bgra, 0, 0).expect("mixed OCR failed");
        assert!(
            out.text.contains("Ask") && out.text.contains("anything"),
            "muted grey text missed/garbled: {}",
            out.text
        );
        assert!(
            out.text.contains("Google") && out.text.contains("Chrome"),
            "pastel desktop icon text missed: {}",
            out.text
        );
    }

    #[test]
    fn wide_horizontal_gap_splits_separate_segments() {
        // Render "4096" on left and "5800" far to the right so they never span the empty middle
        let left = render_test_bgra(340, 40, [250, 250, 250], 0x00111111, -18, 16, 10, "4096");
        let right = render_test_bgra(340, 40, [250, 250, 250], 0x00111111, -18, 250, 10, "5800");
        let mut combined = left;
        for (dst, src) in combined.chunks_exact_mut(4).zip(right.chunks_exact(4)) {
            if src[0] < dst[0] {
                dst.copy_from_slice(src);
            }
        }
        let out = extract_text_offline(340, 40, &combined, 0, 0).expect("gap split OCR failed");
        assert!(
            out.line_boxes.len() >= 2,
            "expected separated segments, got: {:?}",
            out.line_boxes
        );
        assert!(out.line_boxes.iter().any(|b| b.text.contains("4096") && b.width < 90.0));
        assert!(out.line_boxes.iter().any(|b| b.text.contains("5800") && b.x > 200.0));
    }
}
