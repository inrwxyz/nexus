// pill.rs — Corner Assistant Pill (bottom-right native Win32 overlay)
//
// macOS-style floating pill: graphite glass, hairline border, soft shadow.
// Per-pixel-alpha layered window (UpdateLayeredWindow) with software anti-aliased rendering.
//  - Fixed pill size, always. Left slot = one animation (equalizer bars while listening/speaking,
//    green check / red cross for results).
//  - Right slot = text. If the text is longer than the slot it scrolls (marquee) with soft edge
//    fades; the pill never grows.
//  - No purple, no rainbow, no 4-dot.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum PillState {
    Hidden,
    Listening,
    Speech(String),
    Action(String, bool), // (command_text, success)
    Quit,
}

pub struct PillShared {
    pub state: PillState,
    pub dirty: bool,
}

pub struct CornerPill {
    pub shared: Arc<Mutex<PillShared>>,
}

impl CornerPill {
    pub fn new() -> Self {
        CornerPill {
            shared: Arc::new(Mutex::new(PillShared {
                state: PillState::Hidden,
                dirty: false,
            })),
        }
    }

    pub fn show_listening(&self) {
        self.set(PillState::Listening);
        let shared = self.shared.clone();
        thread::spawn(move || {
            // Auto-timeout: If user said wake word and says nothing, hide after 4.2 seconds
            thread::sleep(Duration::from_millis(4200));
            let mut s = shared.lock().unwrap();
            if matches!(s.state, PillState::Listening) {
                s.state = PillState::Hidden;
                s.dirty = true;
            }
        });
    }

    pub fn show_speech(&self, text: &str) {
        self.set(PillState::Speech(text.to_string()));
    }

    pub fn show_action(&self, cmd: &str, success: bool) {
        self.set(PillState::Action(cmd.to_string(), success));
        let shared = self.shared.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(2400));
            let mut s = shared.lock().unwrap();
            if matches!(s.state, PillState::Action(..)) {
                s.state = PillState::Hidden;
                s.dirty = true;
            }
        });
    }

    pub fn hide(&self) {
        self.set(PillState::Hidden);
    }

    pub fn destroy(&self) {
        self.set(PillState::Quit);
    }

    fn set(&self, new_state: PillState) {
        let mut s = self.shared.lock().unwrap();
        if s.state == PillState::Quit && new_state != PillState::Quit {
            return;
        }
        if s.state != new_state {
            s.state = new_state;
            s.dirty = true;
        }
    }
}

// ─── Layout (logical px, scaled by DPI at runtime) ──────────────────────────────────
const PW: f32 = 252.0; // pill width  (fixed)
const PH: f32 = 40.0; // pill height (fixed)
const MARGIN: f32 = 30.0; // transparent room around the pill for the shadow
const WIN_W: f32 = PW + MARGIN * 2.0;
const WIN_H: f32 = PH + MARGIN * 2.0;
const ICON_CX: f32 = 24.0; // animation centre, from pill left
const TEXT_X: f32 = 43.0; // text start, from pill left
const TEXT_R: f32 = 16.0; // right padding
const FONT_PX: i32 = 14; // logical font height

// ─── Platform independent software renderer ─────────────────────────────────────────

/// Anti-aliased text coverage (0..255), rasterised once per string by the platform layer.
pub struct TextBitmap {
    pub w: usize,
    pub h: usize,
    pub cov: Vec<u8>,
}

struct Canvas {
    w: usize,
    h: usize,
    s: f32,
    px: Vec<[f32; 4]>, // premultiplied RGBA 0..1
}

impl Canvas {
    fn new(w: usize, h: usize, s: f32) -> Self {
        Canvas { w, h, s, px: vec![[0.0; 4]; w * h] }
    }

    fn clear(&mut self) {
        for p in self.px.iter_mut() {
            *p = [0.0; 4];
        }
    }

    #[inline]
    fn over(&mut self, x: usize, y: usize, rgb: [f32; 3], a: f32) {
        if a <= 0.0 {
            return;
        }
        let p = &mut self.px[y * self.w + x];
        let ia = 1.0 - a;
        p[0] = rgb[0] * a + p[0] * ia;
        p[1] = rgb[1] * a + p[1] * ia;
        p[2] = rgb[2] * a + p[2] * ia;
        p[3] = a + p[3] * ia;
    }

    /// Paint a shape described by a signed distance function (logical units, <0 inside).
    fn paint<F, G>(&mut self, bbox: (f32, f32, f32, f32), sdf: F, color: G)
    where
        F: Fn(f32, f32) -> f32,
        G: Fn(f32, f32) -> ([f32; 3], f32),
    {
        let s = self.s;
        let xa = (bbox.0 * s).floor().max(0.0) as usize;
        let ya = (bbox.1 * s).floor().max(0.0) as usize;
        let xb = ((bbox.2 * s).ceil().max(0.0) as usize).min(self.w);
        let yb = ((bbox.3 * s).ceil().max(0.0) as usize).min(self.h);
        for y in ya..yb {
            for x in xa..xb {
                let lx = (x as f32 + 0.5) / s;
                let ly = (y as f32 + 0.5) / s;
                let cov = (0.5 - sdf(lx, ly) * s).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let (rgb, a) = color(lx, ly);
                    self.over(x, y, rgb, cov * a);
                }
            }
        }
    }
}

fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (pax, pay) = (px - ax, py - ay);
    let (bax, bay) = (bx - ax, by - ay);
    let h = ((pax * bax + pay * bay) / (bax * bax + bay * bay)).clamp(0.0, 1.0);
    ((pax - bax * h).powi(2) + (pay - bay * h).powi(2)).sqrt()
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Equalizer bars — the single animation in front of the pill.
/// `energetic` = user is speaking / command captured, otherwise a calm breathing wave.
fn draw_bars(c: &mut Canvas, cx: f32, cy: f32, t: f32, age: f32, energetic: bool, vis: f32) {
    let n = 5;
    let gap = 4.9;
    let w = 2.8;
    let ramp = smoothstep(age / 0.35);
    for i in 0..n {
        let fi = i as f32;
        let x = cx + (fi - (n as f32 - 1.0) / 2.0) * gap;
        let h = if energetic {
            5.0 + 11.0
                * ramp
                * ((t * 7.5 + fi * 1.9).sin().abs() * (0.65 + 0.35 * (t * 2.1 + fi * 0.8).sin()))
        } else {
            4.5 + 6.0 * ((t * 3.0 + fi * 0.85).sin() * 0.5 + 0.5)
        };
        let h = h.min(19.0);
        let (hx, hy) = (w / 2.0, h / 2.0);
        c.paint(
            (x - hx - 1.0, cy - hy - 1.0, x + hx + 1.0, cy + hy + 1.0),
            |px, py| sd_round_rect(px, py, x, cy, hx, hy, hx),
            |_, _| ([1.0, 1.0, 1.0], 0.95 * vis),
        );
    }
}

/// Result badge: green check / red cross with a small pop-in.
fn draw_badge(c: &mut Canvas, cx: f32, cy: f32, ok: bool, age: f32, vis: f32) {
    let k = ease_out_back(age / 0.30).max(0.0);
    let r = 9.5 * k;
    let col = if ok {
        [0x30 as f32 / 255.0, 0xd1 as f32 / 255.0, 0x58 as f32 / 255.0]
    } else {
        [1.0, 0x45 as f32 / 255.0, 0x3a as f32 / 255.0]
    };
    c.paint(
        (cx - 14.0, cy - 14.0, cx + 14.0, cy + 14.0),
        |px, py| ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r,
        |_, _| (col, vis),
    );
    let sw = 1.15 * k;
    if ok {
        let (a, b, d) = ((-4.2, 0.3), (-1.4, 3.3), (4.4, -3.3));
        c.paint(
            (cx - 8.0, cy - 8.0, cx + 8.0, cy + 8.0),
            |px, py| {
                let s1 = sd_segment(px, py, cx + a.0 * k, cy + a.1 * k, cx + b.0 * k, cy + b.1 * k);
                let s2 = sd_segment(px, py, cx + b.0 * k, cy + b.1 * k, cx + d.0 * k, cy + d.1 * k);
                s1.min(s2) - sw
            },
            |_, _| ([1.0, 1.0, 1.0], vis),
        );
    } else {
        let q = 3.3 * k;
        c.paint(
            (cx - 8.0, cy - 8.0, cx + 8.0, cy + 8.0),
            |px, py| {
                let s1 = sd_segment(px, py, cx - q, cy - q, cx + q, cy + q);
                let s2 = sd_segment(px, py, cx + q, cy - q, cx - q, cy + q);
                s1.min(s2) - sw
            },
            |_, _| ([1.0, 1.0, 1.0], vis),
        );
    }
}

fn label_for(state: &PillState) -> (String, f32) {
    match state {
        PillState::Listening => ("Listening".to_string(), 0.85),
        PillState::Speech(t) => (t.trim().chars().take(140).collect(), 1.0),
        PillState::Action(c, true) => (c.trim().chars().take(140).collect(), 1.0),
        PillState::Action(_, false) => ("Unknown command".to_string(), 1.0),
        PillState::Hidden | PillState::Quit => (String::new(), 1.0),
    }
}

/// Draw one full frame into the canvas.
/// vis: 0..1 opacity, grow: 0..~1.1 drop/spring progress (can overshoot),
/// age: seconds since this state appeared, t: global seconds.
fn render_frame(
    c: &mut Canvas,
    state: &PillState,
    vis: f32,
    grow: f32,
    age: f32,
    t: f32,
    text: Option<&TextBitmap>,
    text_alpha: f32,
) {
    c.clear();
    let s = c.s;
    // Drop-in: starts as a small drop at the right edge (a bit above), falls into place,
    // stretches into the full pill with a little spring overshoot, then settles.
    let g = grow;
    let pw = (PH + (PW - PH) * g).max(PH);
    let right = MARGIN + PW + (1.0 - g) * 24.0;
    let left = right - pw;
    let oy = MARGIN - (1.0 - g) * 14.0;
    let cx = (left + right) / 2.0;
    let cy = oy + PH / 2.0;
    let (hx, hy, r) = (pw / 2.0, PH / 2.0, PH / 2.0);
    // content (icon + text) appears once the pill has opened up
    let cvis = vis * smoothstep((g - 0.35) / 0.45);

    // 1) soft drop shadow
    {
        let sy = 7.0;
        let sigma = 8.0_f32;
        c.paint(
            (0.0, 0.0, WIN_W, WIN_H),
            |px, py| sd_round_rect(px, py - sy, cx, cy, hx, hy, r) - 100.0, // force full coverage
            |px, py| {
                let d = sd_round_rect(px, py - sy, cx, cy, hx - 2.0, hy - 2.0, r - 2.0).max(0.0);
                let a = 0.30 * (-(d * d) / (2.0 * sigma * sigma)).exp();
                ([0.0, 0.0, 0.0], a * vis)
            },
        );
    }

    // 2) graphite glass body (vertical gradient)
    c.paint(
        (left - 2.0, oy - 2.0, right + 2.0, oy + PH + 2.0),
        |px, py| sd_round_rect(px, py, cx, cy, hx, hy, r),
        |_, py| {
            let k = ((py - oy) / PH).clamp(0.0, 1.0);
            let v = lerp(0x30 as f32, 0x1e as f32, k) / 255.0;
            ([v, v, v + 0.004], 0.60 * vis)
        },
    );

    // 3) hairline border (brighter at the top, like macOS)
    c.paint(
        (left - 2.0, oy - 2.0, right + 2.0, oy + PH + 2.0),
        |px, py| {
            let d = sd_round_rect(px, py, cx, cy, hx, hy, r);
            // ring of 1px just inside the edge
            (d + 0.5).abs() - 0.5
        },
        |_, py| {
            let k = ((py - oy) / PH).clamp(0.0, 1.0);
            ([1.0, 1.0, 1.0], lerp(0.26, 0.09, k) * vis)
        },
    );

    // 4) animation slot
    let icx = left + ICON_CX;
    match state {
        PillState::Listening => draw_bars(c, icx, cy, t, age, false, cvis),
        PillState::Speech(_) => draw_bars(c, icx, cy, t, age, true, cvis),
        PillState::Action(_, ok) => draw_badge(c, icx, cy, *ok, age, cvis),
        PillState::Hidden | PillState::Quit => {}
    }

    // 5) text slot (marquee when too long, pill size never changes)
    if let Some(tb) = text {
        let x0 = ((left + TEXT_X) * s).round() as isize;
        // final text slot width (constant, so marquee never flickers while the pill opens)
        let avail = (((PW - TEXT_X - TEXT_R) * s).round() as isize).max(1);
        // never draw outside the (still growing) pill
        let x1 = (x0 + avail).min(((right - TEXT_R) * s).round() as isize);
        let tw = tb.w as isize;
        let overflow = tw > avail;
        let gap = (40.0 * s) as isize;
        let period = tw + gap;
        let pos: isize = if overflow {
            let run = (age - 1.0).max(0.0); // short pause before scrolling
            ((run * 42.0 * s) as isize) % period.max(1)
        } else {
            0
        };
        let fade_l = (12.0 * s).max(1.0);
        let fade_r = (18.0 * s).max(1.0);
        let left_strength = if overflow { smoothstep(pos as f32 / fade_l) } else { 0.0 };
        let in_a = smoothstep(age / 0.20); // text fades in
        let modulo = if overflow { period.max(1) } else { isize::MAX };
        let top = ((cy * s) - tb.h as f32 / 2.0).round() as isize;
        for ty in 0..tb.h as isize {
            let y = top + ty;
            if y < 0 || y >= c.h as isize {
                continue;
            }
            for x in x0..x1 {
                let u = x - x0;
                let src = (u + pos) % modulo;
                if src < 0 || src >= tw {
                    continue;
                }
                let cov = (tb.cov[(ty as usize) * tb.w + src as usize] as f32 / 255.0).powf(0.62);
                if cov <= 0.0 {
                    continue;
                }
                let mut m = 1.0;
                if overflow {
                    let l = u as f32 / fade_l;
                    m *= 1.0 - left_strength * (1.0 - smoothstep(l));
                    let rr = (avail - u) as f32 / fade_r;
                    m *= smoothstep(rr);
                }
                c.over(
                    x as usize,
                    y as usize,
                    [1.0, 1.0, 1.0],
                    cov * m * in_a * text_alpha * cvis,
                );
            }
        }
    }
}

// ─── Windows Win32 overlay ───────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
pub fn spawn_pill_window(shared: Arc<Mutex<PillShared>>) {
    thread::spawn(move || unsafe { pill_thread(shared) });
}

/// Rasterise a string with GDI grayscale anti-aliasing and return its coverage map.
#[cfg(target_os = "windows")]
unsafe fn rasterize_text(text: &str, px_h: i32) -> TextBitmap {
    use windows_sys::Win32::Foundation::SIZE;
    use windows_sys::Win32::Graphics::Gdi::*;

    let screen = GetDC(std::ptr::null_mut());
    let dc = CreateCompatibleDC(screen);

    let font_name: Vec<u16> = "Segoe UI Variable\0".encode_utf16().collect();
    // quality 4 = ANTIALIASED_QUALITY (grayscale, so we can use it as alpha)
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
    bmi.bmiHeader.biHeight = -th; // top-down
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
            cov[i] = ((p >> 8) & 0xff) as u8; // green channel = grayscale coverage
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

    // DPI scale
    let screen = GetDC(std::ptr::null_mut());
    let dpi = GetDeviceCaps(screen, LOGPIXELSX as i32);
    let s: f32 = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    let ww = (WIN_W * s).round() as i32;
    let wh = (WIN_H * s).round() as i32;

    // window position: pill sits bottom-right clearly above the taskbar/tray
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

    // 32-bit top-down DIB the whole frame is composed into
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

    let mut shown = PillState::Hidden; // last visible state (kept while fading out)
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

        // fade in a bit quicker than out
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

        // premultiplied float -> premultiplied BGRA
        for (dst, p) in frame.iter_mut().zip(canvas.px.iter()) {
            let a = (p[3] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let r = (p[0] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let g = (p[1] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            let b = (p[2] * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
            *dst = (a << 24) | (r << 16) | (g << 8) | b;
        }

        if !visible {
            let (nx, ny) = place();
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                nx,
                ny,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_stays_above_taskbar_even_when_workarea_equals_screen() {
        let full = windows_sys::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let (_, y) = compute_pill_origin(full, 1080, 1.0);
        let visible_bottom = y + ((PH + MARGIN).round() as i32);
        assert!(
            visible_bottom <= 1032 - 20,
            "pill visible bottom {visible_bottom} must clear 48px Win11 taskbar"
        );
    }
}
