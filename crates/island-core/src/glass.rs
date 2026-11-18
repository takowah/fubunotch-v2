// Liquid Glass (Settings → Appearance → Look), the pixels: what is behind
// the island, blurred and saturated as CSS `backdrop-filter: blur()
// saturate()`, bent near the rounded edge like thick glass (the backdrop is
// sampled a few pixels further out the closer a pixel is to the edge, along
// the edge's normal), a light or dark tint so text reads, a bright rim
// (stronger on the top edge) and a soft sheen near the top. Cut to the
// island's rounded shape here, since the software renderer clips square.
//
// Pure: the app hands in a small (already downscaled) picture of the screen
// around the island and gets the island's RGBA back. Buffers are kept
// between frames.

/// What the backdrop is blurred by, in logical px (CSS blur's radius = sigma).
pub const BLUR: f32 = 18.0;
/// CSS saturate(): 1 = as is.
pub const SATURATE: f32 = 1.75;
/// How far the edge bends the backdrop (logical px) and over how wide a band.
pub const LENS: f32 = 12.0;
pub const LENS_BAND: f32 = 18.0;
/// Room captured around the island (logical px), for the lens to reach into
/// and so the blur doesn't darken at the island's edges.
pub const MARGIN: f32 = 14.0;
/// The tint over the blurred backdrop: black on a dark one, white on a light one.
const DARK_TINT: f32 = 0.36;
const LIGHT_TINT: f32 = 0.50;
/// Backdrop brightness (0..1) above which the glass goes light and the ink
/// dark, and below which it goes dark again (a gap, so it doesn't flicker).
const LIGHT_ABOVE: f32 = 0.58;
const DARK_BELOW: f32 = 0.48;

type Px = [f32; 3];

/// Where the island sits in the captured picture and how it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// The island in physical pixels.
    pub w: usize,
    pub h: usize,
    /// Its corner radius, physical px.
    pub radius: f32,
    /// Physical px per logical px.
    pub scale: f32,
    /// The island's top-left inside the capture, physical px.
    pub ox: f32,
    pub oy: f32,
    /// Physical px per captured px (the capture is downscaled).
    pub factor: f32,
}

#[derive(Default)]
pub struct Glass {
    src: Vec<Px>,
    sharp: Vec<Px>,
    blur: Vec<Px>,
    tmp: Vec<Px>,
    sw: usize,
    sh: usize,
}

impl Glass {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the captured picture (RGBA, or BGRA as GDI gives it) and
    /// returns a cheap hash of it, so an unchanged screen costs nothing more.
    pub fn load(&mut self, px: &[u8], w: usize, h: usize, bgra: bool) -> u64 {
        self.sw = w;
        self.sh = h;
        self.src.clear();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for p in px.chunks_exact(4).take(w * h) {
            let (r, b) = if bgra { (p[2], p[0]) } else { (p[0], p[2]) };
            self.src.push([r as f32 / 255.0, p[1] as f32 / 255.0, b as f32 / 255.0]);
            // Two low bits dropped: noise from scaling doesn't count as a change.
            for v in [r >> 2, p[1] >> 2, b >> 2] {
                hash = (hash ^ v as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        self.src.resize(w * h, [0.0; 3]);
        hash
    }

    /// Draws the island into `out` (w * h * 4 bytes, straight RGBA).
    /// `was_light`: the last frame's choice. Returns whether the glass is
    /// light (dark ink) now.
    /// `force`: Some(true) light, Some(false) dark, None = by the backdrop.
    pub fn render(&mut self, s: &Shape, was_light: bool, force: Option<bool>, out: &mut [u8]) -> bool {
        let (sw, sh) = (self.sw, self.sh);
        if sw == 0 || sh == 0 || s.w == 0 || s.h == 0 || out.len() < s.w * s.h * 4 {
            return was_light;
        }
        // The edge's source: only the capture's own softness, so what the
        // lens bends still shows.
        self.sharp.clone_from(&self.src);
        self.blur.clone_from(&self.src);
        blur(&mut self.blur, &mut self.tmp, sw, sh, BLUR * s.scale / s.factor);
        for p in self.sharp.iter_mut().chain(self.blur.iter_mut()) {
            *p = saturate(*p, SATURATE);
        }
        let light = force.unwrap_or_else(|| backdrop_is_light(self.mean_luma(s), was_light));

        let band = (LENS_BAND * s.scale).min(s.w.min(s.h) as f32 * 0.5).max(1.0);
        let lens = LENS * s.scale;
        let sheen_h = (s.h as f32 * 0.5).min(36.0 * s.scale).max(1.0);
        for y in 0..s.h {
            for x in 0..s.w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (d, nx, ny) = round_rect(fx, fy, s.w as f32, s.h as f32, s.radius);
                let o = (y * s.w + x) * 4;
                let cover = (d + 0.5).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    out[o..o + 4].copy_from_slice(&[0, 0, 0, 0]);
                    continue;
                }
                // The lens: sample further out near the edge.
                let push = refraction(d, band, lens);
                let (sx, sy) = (s.ox + fx + nx * push, s.oy + fy + ny * push);
                let (u, v) = (sx / s.factor - 0.5, sy / s.factor - 0.5);
                let e = edge_weight(d, band);
                let soft = sample(&self.blur, sw, sh, u, v);
                let crisp = sample(&self.sharp, sw, sh, u, v);
                let mut c = mix(soft, crisp, 0.85 * e);
                // The tint, thinner at the edge: the lens is clearer glass.
                let k = 1.0 - 0.45 * e;
                c = if light { mix(c, [1.0; 3], LIGHT_TINT * k) } else { mix(c, [0.0; 3], DARK_TINT * k) };
                // The sheen: a soft light over the top.
                let t = (1.0 - fy / sheen_h).max(0.0);
                let mut white = 0.10 * t * t;
                // The rim: a 1 px line of light, strongest where the edge
                // faces up and to the left, a weaker reflection opposite.
                let facing = -0.45 * nx - 0.9 * ny;
                let line = (1.0 - d / (1.2 * s.scale)).clamp(0.0, 1.0);
                // On light glass the edge also takes a faint grey line, so
                // it shows against a white page; the rim lights it on top.
                if light {
                    c = mix(c, [0.0; 3], 0.16 * line);
                }
                white += line * (0.20 + 0.55 * facing.max(0.0) + 0.22 * (-facing).max(0.0));
                // The glass' thickness: a faint glow inside the edge.
                let glow = (1.0 - d / (6.0 * s.scale)).clamp(0.0, 1.0);
                white += 0.07 * glow * glow * (0.4 + facing.max(0.0));
                c = mix(c, [1.0; 3], white.min(0.9));
                out[o] = (c[0] * 255.0).round() as u8;
                out[o + 1] = (c[1] * 255.0).round() as u8;
                out[o + 2] = (c[2] * 255.0).round() as u8;
                out[o + 3] = (cover * 255.0).round() as u8;
            }
        }
        light
    }

    /// The blurred backdrop's mean brightness inside the island, 0..1.
    fn mean_luma(&self, s: &Shape) -> f32 {
        let x0 = (s.ox / s.factor).floor().max(0.0) as usize;
        let y0 = (s.oy / s.factor).floor().max(0.0) as usize;
        let x1 = (((s.ox + s.w as f32) / s.factor).ceil() as usize).min(self.sw);
        let y1 = (((s.oy + s.h as f32) / s.factor).ceil() as usize).min(self.sh);
        let (mut sum, mut n) = (0.0f32, 0usize);
        for y in y0..y1 {
            for x in x0..x1 {
                sum += luma(self.blur[y * self.sw + x]);
                n += 1;
            }
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }
}

/// Light glass (and dark ink) over a backdrop this bright, given the last choice.
pub fn backdrop_is_light(luma: f32, was_light: bool) -> bool {
    if was_light { luma > DARK_BELOW } else { luma > LIGHT_ABOVE }
}

/// Rec. 709 brightness of a gamma-encoded colour, 0..1.
pub fn luma(c: Px) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// CSS saturate(s): away from (or toward) the colour's grey.
pub fn saturate(c: Px, s: f32) -> Px {
    let l = luma(c);
    [(l + (c[0] - l) * s).clamp(0.0, 1.0), (l + (c[1] - l) * s).clamp(0.0, 1.0), (l + (c[2] - l) * s).clamp(0.0, 1.0)]
}

fn mix(a: Px, b: Px, t: f32) -> Px {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// How far inside the rounded rectangle (w x h, corner radius r) the point
/// is (negative: outside), and the edge's outward normal there.
pub fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> (f32, f32, f32) {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let (px, py) = (x - w / 2.0, y - h / 2.0);
    let (qx, qy) = (px.abs() - (w / 2.0 - r), py.abs() - (h / 2.0 - r));
    let (sx, sy) = (if px < 0.0 { -1.0 } else { 1.0 }, if py < 0.0 { -1.0 } else { 1.0 });
    if qx > 0.0 && qy > 0.0 {
        // In a corner: away from the corner's centre.
        let len = (qx * qx + qy * qy).sqrt();
        (r - len, sx * qx / len, sy * qy / len)
    } else if qx > qy {
        (r - qx, sx, 0.0)
    } else {
        (r - qy, 0.0, sy)
    }
}

/// How much of the lens a pixel `d` px inside the edge gets: 1 at the edge,
/// easing to 0 at `band`.
pub fn edge_weight(d: f32, band: f32) -> f32 {
    let t = (1.0 - d.max(0.0) / band).clamp(0.0, 1.0);
    t * t
}

/// The lens' outward push (px) for a pixel `d` px inside the edge.
pub fn refraction(d: f32, band: f32, amount: f32) -> f32 {
    amount * edge_weight(d, band)
}

/// Bilinear sample, clamped at the picture's edges.
fn sample(buf: &[Px], w: usize, h: usize, u: f32, v: f32) -> Px {
    let u = u.clamp(0.0, (w - 1) as f32);
    let v = v.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (u.floor() as usize, v.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (u - x0 as f32, v - y0 as f32);
    let top = mix(buf[y0 * w + x0], buf[y0 * w + x1], tx);
    let bottom = mix(buf[y1 * w + x0], buf[y1 * w + x1], tx);
    mix(top, bottom, ty)
}

/// A gaussian blur of `sigma` px, as three box blurs.
pub fn blur(buf: &mut [Px], tmp: &mut Vec<Px>, w: usize, h: usize, sigma: f32) {
    if sigma < 0.5 {
        return;
    }
    // Box width whose three passes give this sigma: sqrt(12 s² / 3 + 1).
    let width = (4.0 * sigma * sigma + 1.0).sqrt();
    let r = ((width - 1.0) / 2.0).round().max(1.0) as usize;
    for _ in 0..3 {
        box_blur(buf, tmp, w, h, r);
    }
}

/// One box blur pass of radius `r`, rows then columns, edges clamped.
pub fn box_blur(buf: &mut [Px], tmp: &mut Vec<Px>, w: usize, h: usize, r: usize) {
    if w == 0 || h == 0 || r == 0 || buf.len() < w * h {
        return;
    }
    tmp.clear();
    tmp.resize(w * h, [0.0; 3]);
    let n = (2 * r + 1) as f32;
    // Rows into tmp.
    for y in 0..h {
        let row = &buf[y * w..(y + 1) * w];
        let at = |i: isize| row[i.clamp(0, w as isize - 1) as usize];
        let mut acc = [0.0f32; 3];
        for i in -(r as isize)..=(r as isize) {
            let p = at(i);
            acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
        }
        for x in 0..w {
            tmp[y * w + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            let (add, sub) = (at(x as isize + r as isize + 1), at(x as isize - r as isize));
            acc = [acc[0] + add[0] - sub[0], acc[1] + add[1] - sub[1], acc[2] + add[2] - sub[2]];
        }
    }
    // Columns back into buf.
    for x in 0..w {
        let at = |i: isize| tmp[i.clamp(0, h as isize - 1) as usize * w + x];
        let mut acc = [0.0f32; 3];
        for i in -(r as isize)..=(r as isize) {
            let p = at(i);
            acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
        }
        for y in 0..h {
            buf[y * w + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            let (add, sub) = (at(y as isize + r as isize + 1), at(y as isize - r as isize));
            acc = [acc[0] + add[0] - sub[0], acc[1] + add[1] - sub[1], acc[2] + add[2] - sub[2]];
        }
    }
}

/// An RGBA picture box-averaged down by `f` (each side), for a backdrop
/// that came at full size (a picture instead of the screen).
pub fn downscale(px: &[u8], w: usize, h: usize, f: usize) -> (Vec<u8>, usize, usize) {
    let f = f.max(1);
    let (dw, dh) = (w.div_ceil(f), h.div_ceil(f));
    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        for dx in 0..dw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for y in dy * f..((dy + 1) * f).min(h) {
                for x in dx * f..((dx + 1) * f).min(w) {
                    let i = (y * w + x) * 4;
                    for k in 0..4 {
                        acc[k] += px[i + k] as u32;
                    }
                    n += 1;
                }
            }
            let o = (dy * dw + dx) * 4;
            for k in 0..4 {
                out[o + k] = (acc[k] / n.max(1)) as u8;
            }
        }
    }
    (out, dw, dh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blur_keeps_flat_colour_and_spreads_a_dot() {
        let (w, h) = (21, 21);
        let mut flat = vec![[0.3, 0.5, 0.7]; w * h];
        let mut tmp = Vec::new();
        blur(&mut flat, &mut tmp, w, h, 3.0);
        assert!(flat.iter().all(|p| (p[0] - 0.3).abs() < 1e-4 && (p[2] - 0.7).abs() < 1e-4));
        let mut dot = vec![[0.0; 3]; w * h];
        dot[10 * w + 10] = [1.0; 3];
        blur(&mut dot, &mut tmp, w, h, 3.0);
        let total: f32 = dot.iter().map(|p| p[0]).sum();
        assert!((total - 1.0).abs() < 1e-3, "light is kept: {total}");
        assert!(dot[10 * w + 10][0] < 0.1 && dot[10 * w + 13][0] > 0.0, "spread out");
        assert!(dot[10 * w + 10][0] > dot[10 * w + 14][0], "strongest in the middle");
    }

    #[test]
    fn saturate_moves_away_from_grey_and_keeps_grey() {
        assert_eq!(saturate([0.5, 0.5, 0.5], 1.8), [0.5, 0.5, 0.5]);
        let s = saturate([0.6, 0.4, 0.4], 1.8);
        assert!(s[0] > 0.6 && s[1] < 0.4, "{s:?}");
        assert!((luma(s) - luma([0.6, 0.4, 0.4])).abs() < 1e-4, "same brightness");
    }

    #[test]
    fn the_rounded_shape_knows_inside_outside_and_its_normals() {
        // A 100 x 40 pill (radius 20).
        let (d, nx, ny) = round_rect(50.0, 0.5, 100.0, 40.0, 20.0);
        assert!((d - 0.5).abs() < 1e-4 && nx == 0.0 && ny == -1.0, "top edge faces up");
        let (d, _, ny) = round_rect(50.0, 39.5, 100.0, 40.0, 20.0);
        assert!((d - 0.5).abs() < 1e-4 && ny == 1.0, "bottom faces down");
        assert!(round_rect(50.0, 20.0, 100.0, 40.0, 20.0).0 >= 19.9, "the middle is deep inside");
        assert!(round_rect(1.0, 1.0, 100.0, 40.0, 20.0).0 < 0.0, "the square corner is outside");
        let (d, nx, ny) = round_rect(20.0 - 14.142, 20.0 - 14.142, 100.0, 40.0, 20.0);
        assert!(d.abs() < 0.01 && nx < -0.7 && ny < -0.7, "on the arc, facing up-left: {d} {nx} {ny}");
    }

    #[test]
    fn the_lens_pushes_hardest_at_the_edge_and_not_inside() {
        assert_eq!(refraction(0.0, 20.0, 10.0), 10.0);
        assert!(refraction(5.0, 20.0, 10.0) < 10.0 && refraction(5.0, 20.0, 10.0) > refraction(10.0, 20.0, 10.0));
        assert_eq!(refraction(20.0, 20.0, 10.0), 0.0);
        assert_eq!(refraction(50.0, 20.0, 10.0), 0.0);
    }

    #[test]
    fn light_and_dark_switch_with_a_gap() {
        assert!(!backdrop_is_light(0.5, false));
        assert!(backdrop_is_light(0.5, true), "stays light in the gap");
        assert!(backdrop_is_light(0.9, false));
        assert!(!backdrop_is_light(0.2, true));
    }

    fn shape(w: usize, h: usize) -> Shape {
        // The island 10 px inside a capture at quarter size.
        Shape { w, h, radius: h as f32 / 2.0, scale: 1.0, ox: 10.0, oy: 10.0, factor: 4.0 }
    }

    #[test]
    fn the_island_is_cut_round_and_tinted_by_the_backdrop() {
        let s = shape(120, 40);
        let (cw, ch) = (140usize.div_ceil(4), 60usize.div_ceil(4));
        let mut g = Glass::new();
        let mut out = vec![0u8; s.w * s.h * 4];
        // A white backdrop: light glass.
        g.load(&vec![255u8; cw * ch * 4], cw, ch, false);
        assert!(g.render(&s, false, None, &mut out));
        assert_eq!(out[3], 0, "the corner is cut");
        let mid = (20 * s.w + 60) * 4;
        assert_eq!(out[mid + 3], 255);
        assert!(out[mid] > 200, "light glass over white");
        // A black backdrop: dark glass, a bright rim on top.
        let black: Vec<u8> = [0u8, 0, 0, 255].repeat(cw * ch);
        g.load(&black, cw, ch, false);
        assert!(!g.render(&s, true, None, &mut out));
        assert!(out[mid] < 40, "dark glass over black: {}", out[mid]);
        let top = (60) * 4;
        let bottom = ((s.h - 1) * s.w + 60) * 4;
        assert!(out[top] > out[bottom], "the top rim is the brighter: {} {}", out[top], out[bottom]);
        assert!(out[top] > out[mid] + 40, "a rim: {} {}", out[top], out[mid]);
    }

    #[test]
    fn the_edge_shows_what_is_just_outside() {
        // Red above the island, blue everywhere else: the lens bends the red
        // into the top edge; the middle stays blue.
        let s = shape(120, 40);
        let (cw, ch) = (35usize, 15usize);
        let mut px = Vec::new();
        for y in 0..ch {
            for _ in 0..cw {
                px.extend_from_slice(if y < 2 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        let mut g = Glass::new();
        let mut out = vec![0u8; s.w * s.h * 4];
        g.load(&px, cw, ch, false);
        g.render(&s, false, None, &mut out);
        let at = |x: usize, y: usize| ((y * s.w + x) * 4, ());
        let (top, _) = at(60, 3);
        let (mid, _) = at(60, 20);
        let redness = |i: usize| out[i] as i32 - out[i + 2] as i32;
        assert!(redness(top) > redness(mid), "top {:?} mid {:?}", &out[top..top + 3], &out[mid..mid + 3]);
    }

    #[test]
    fn an_unchanged_screen_hashes_the_same() {
        let mut g = Glass::new();
        let a = vec![100u8; 64];
        let mut b = a.clone();
        b[0] = 101; // noise in the low bits
        assert_eq!(g.load(&a, 4, 4, false), g.load(&b, 4, 4, false));
        b[5] = 200;
        assert_ne!(g.load(&a, 4, 4, false), g.load(&b, 4, 4, false));
    }

    #[test]
    fn downscale_averages_blocks() {
        let px = [[0u8, 0, 0, 255], [200, 200, 200, 255]].concat().repeat(8);
        let (out, w, h) = downscale(&px, 4, 4, 2);
        assert_eq!((w, h), (2, 2));
        assert_eq!(&out[0..4], &[100, 100, 100, 255]);
    }
}
