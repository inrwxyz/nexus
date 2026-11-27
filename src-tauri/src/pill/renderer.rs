//! Platform-independent anti-aliased software renderer for the Corner Assistant Pill.

use super::PillState;

pub(crate) const PW: f32 = 252.0; // pill width (fixed)
pub(crate) const PH: f32 = 40.0; // pill height (fixed)
pub(crate) const MARGIN: f32 = 30.0; // transparent room around pill for shadow
pub(crate) const WIN_W: f32 = PW + MARGIN * 2.0;
pub(crate) const WIN_H: f32 = PH + MARGIN * 2.0;
pub(crate) const ICON_CX: f32 = 24.0;
pub(crate) const TEXT_X: f32 = 43.0;
pub(crate) const TEXT_R: f32 = 16.0;
pub(crate) const FONT_PX: i32 = 14;

/// Anti-aliased text coverage (0..255), rasterized once per string by the platform layer.
pub struct TextBitmap {
    pub w: usize,
    pub h: usize,
    pub cov: Vec<u8>,
}

pub(crate) struct Canvas {
    pub w: usize,
    pub h: usize,
    pub s: f32,
    pub px: Vec<[f32; 4]>, // premultiplied RGBA 0..1
}

impl Canvas {
    pub(crate) fn new(w: usize, h: usize, s: f32) -> Self {
        Self { w, h, s, px: vec![[0.0; 4]; w * h] }
    }

    pub(crate) fn clear(&mut self) {
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

pub(crate) fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

pub(crate) fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

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

pub(crate) fn label_for(state: &PillState) -> (String, f32) {
    match state {
        PillState::Listening => ("Listening".to_string(), 0.85),
        PillState::Speech(t) => (t.trim().chars().take(140).collect(), 1.0),
        PillState::Action(c, true) => (c.trim().chars().take(140).collect(), 1.0),
        PillState::Action(_, false) => ("Unknown command".to_string(), 1.0),
        PillState::Hidden | PillState::Quit => (String::new(), 1.0),
    }
}

/// Draw one full frame into the canvas.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_frame(
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
    let g = grow;
    let pw = (PH + (PW - PH) * g).max(PH);
    let right = MARGIN + PW + (1.0 - g) * 24.0;
    let left = right - pw;
    let oy = MARGIN - (1.0 - g) * 14.0;
    let cx = (left + right) / 2.0;
    let cy = oy + PH / 2.0;
    let (hx, hy, r) = (pw / 2.0, PH / 2.0, PH / 2.0);
    let cvis = vis * smoothstep((g - 0.35) / 0.45);

    {
        let sy = 7.0;
        let sigma = 8.0_f32;
        c.paint(
            (0.0, 0.0, WIN_W, WIN_H),
            |px, py| sd_round_rect(px, py - sy, cx, cy, hx, hy, r) - 100.0,
            |px, py| {
                let d = sd_round_rect(px, py - sy, cx, cy, hx - 2.0, hy - 2.0, r - 2.0).max(0.0);
                let a = 0.30 * (-(d * d) / (2.0 * sigma * sigma)).exp();
                ([0.0, 0.0, 0.0], a * vis)
            },
        );
    }

    c.paint(
        (left - 2.0, oy - 2.0, right + 2.0, oy + PH + 2.0),
        |px, py| sd_round_rect(px, py, cx, cy, hx, hy, r),
        |_, py| {
            let k = ((py - oy) / PH).clamp(0.0, 1.0);
            let v = lerp(0x30 as f32, 0x1e as f32, k) / 255.0;
            ([v, v, v + 0.004], 0.60 * vis)
        },
    );

    c.paint(
        (left - 2.0, oy - 2.0, right + 2.0, oy + PH + 2.0),
        |px, py| {
            let d = sd_round_rect(px, py, cx, cy, hx, hy, r);
            (d + 0.5).abs() - 0.5
        },
        |_, py| {
            let k = ((py - oy) / PH).clamp(0.0, 1.0);
            ([1.0, 1.0, 1.0], lerp(0.26, 0.09, k) * vis)
        },
    );

    let icx = left + ICON_CX;
    match state {
        PillState::Listening => draw_bars(c, icx, cy, t, age, false, cvis),
        PillState::Speech(_) => draw_bars(c, icx, cy, t, age, true, cvis),
        PillState::Action(_, ok) => draw_badge(c, icx, cy, *ok, age, cvis),
        PillState::Hidden | PillState::Quit => {}
    }

    if let Some(tb) = text {
        let x0 = ((left + TEXT_X) * s).round() as isize;
        let avail = (((PW - TEXT_X - TEXT_R) * s).round() as isize).max(1);
        let x1 = (x0 + avail).min(((right - TEXT_R) * s).round() as isize);
        let tw = tb.w as isize;
        let overflow = tw > avail;
        let gap = (40.0 * s) as isize;
        let period = tw + gap;
        let pos: isize = if overflow {
            let run = (age - 1.0).max(0.0);
            ((run * 42.0 * s) as isize) % period.max(1)
        } else {
            0
        };
        let fade_l = (12.0 * s).max(1.0);
        let fade_r = (18.0 * s).max(1.0);
        let left_strength = if overflow { smoothstep(pos as f32 / fade_l) } else { 0.0 };
        let in_a = smoothstep(age / 0.20);
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
                c.over(x as usize, y as usize, [1.0, 1.0, 1.0], cov * m * in_a * text_alpha * cvis);
            }
        }
    }
}
