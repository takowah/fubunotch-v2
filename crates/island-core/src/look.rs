// The island's Look (Settings → Appearance), worked out without any UI: the
// body's colour for each look, which ink reads on it, the wallpaper's colour
// made dark enough for white text, and how warm the colours are at this
// minute of the night. The app turns the results into Slint colours.

pub type Rgb = (u8, u8, u8);

/// "#rrggbb" or "rrggbb" → colour.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// WCAG relative luminance, 0 (black) .. 1 (white).
pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// Dark ink reads better than white on this body (white, yellow, pink...):
/// the ink with the higher contrast wins.
pub fn wants_dark_ink(body: Rgb) -> bool {
    let l = luminance(body);
    // Contrast against black (l + .05) / .05 vs against white 1.05 / (l + .05).
    (l + 0.05) / 0.05 > 1.05 / (l + 0.05)
}

/// The wallpaper's colour from its pixels (RGBA): an average that leans on
/// the colourful pixels (a grey sky doesn't wash out a red roof), then made
/// dark enough for the island's white text. None for no pixels.
pub fn wallpaper_color(rgba: &[u8]) -> Option<Rgb> {
    let (mut r, mut g, mut b, mut weight) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for px in rgba.chunks_exact(4) {
        let (pr, pg, pb) = (px[0] as f64, px[1] as f64, px[2] as f64);
        let max = pr.max(pg).max(pb);
        let sat = if max > 0.0 { (max - pr.min(pg).min(pb)) / max } else { 0.0 };
        let w = sat * sat + 0.08;
        r += pr * w;
        g += pg * w;
        b += pb * w;
        weight += w;
    }
    if weight == 0.0 {
        return None;
    }
    Some(darkened((r / weight) as f32, (g / weight) as f32, (b / weight) as f32))
}

/// The same hue at most 30 % bright and at most 70 % saturated.
fn darkened(r: f32, g: f32, b: f32) -> Rgb {
    let max = r.max(g).max(b).max(1.0);
    let min = r.min(g).min(b);
    let sat = ((max - min) / max).min(0.7);
    let value = (max / 255.0).min(0.30) * 255.0;
    let low = value * (1.0 - sat);
    let span = (max - min).max(1e-3);
    let at = |c: f32| if max - min < 1e-3 { value.round() as u8 } else { (low + (c - min) / span * (value - low)).round().clamp(0.0, 255.0) as u8 };
    (at(r), at(g), at(b))
}

/// How far into the night `now` is, 0 (day) .. 1 (night): the warmth eases
/// in over 45 minutes from sunset and out over 45 minutes from sunrise.
/// Minutes after midnight; sunrise before sunset.
pub fn night_phase(now: f32, sunset: f32, sunrise: f32) -> f32 {
    const EASE: f32 = 45.0;
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if now >= sunset {
        smooth((now - sunset) / EASE)
    } else if now >= sunrise {
        // Daytime, or the first 45 minutes after sunrise still going back.
        1.0 - smooth((now - sunrise) / EASE)
    } else {
        1.0
    }
}

/// A colour moved warmer by `w` (0..1): less blue, a little less green, as
/// Windows' night light. A dark body also takes on some amber, else black
/// would stay black.
pub fn warm(c: Rgb, w: f32) -> Rgb {
    let w = w.clamp(0.0, 1.0);
    let (r, g, b) = (c.0 as f32, c.1 as f32 * (1.0 - 0.14 * w), c.2 as f32 * (1.0 - 0.45 * w));
    let dark = 1.0 - (r.max(g).max(b) / 255.0);
    // The amber a black body moves toward.
    let amber = (64.0, 36.0, 10.0);
    let k = 0.55 * w * dark;
    let m = |x: f32, a: f32| (x + (a - x).max(0.0) * k).round().clamp(0.0, 255.0) as u8;
    (m(r, amber.0), m(g, amber.1), m(b, amber.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        assert_eq!(parse_hex("#1F2a52"), Some((0x1f, 0x2a, 0x52)));
        assert_eq!(parse_hex("ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(hex((0x1f, 0x2a, 0x52)), "#1f2a52");
    }

    #[test]
    fn light_bodies_take_dark_ink() {
        for light in [(255, 255, 255), (255, 214, 10), (255, 159, 199)] {
            assert!(wants_dark_ink(light), "{light:?}");
        }
        for dark in [(0x0b, 0x0b, 0x0d), (0x1f, 0x2a, 0x52), (0x5e, 0x2a, 0x84), (0x0f, 0x4c, 0x4a), (0x5c, 0x16, 0x24)] {
            assert!(!wants_dark_ink(dark), "{dark:?}");
        }
    }

    #[test]
    fn wallpaper_colour_keeps_its_hue_and_is_dark() {
        let blue = [40u8, 90, 220, 255].repeat(100);
        let (r, g, b) = wallpaper_color(&blue).unwrap();
        assert!(b > g && g > r, "{r} {g} {b}");
        assert!(!wants_dark_ink((r, g, b)));
        assert!(b <= 77);
        // A white wallpaper still gives a body white text reads on.
        let white = [250u8, 250, 250, 255].repeat(100);
        let c = wallpaper_color(&white).unwrap();
        assert!(!wants_dark_ink(c), "{c:?}");
        assert_eq!(wallpaper_color(&[]), None);
    }

    #[test]
    fn night_eases_in_after_sunset_and_out_after_sunrise() {
        let (rise, set) = (6.0 * 60.0, 18.0 * 60.0);
        assert_eq!(night_phase(12.0 * 60.0, set, rise), 0.0);
        assert_eq!(night_phase(set, set, rise), 0.0);
        let half = night_phase(set + 22.5, set, rise);
        assert!((half - 0.5).abs() < 0.01, "{half}");
        assert_eq!(night_phase(set + 45.0, set, rise), 1.0);
        assert_eq!(night_phase(23.0 * 60.0, set, rise), 1.0);
        assert_eq!(night_phase(3.0 * 60.0, set, rise), 1.0);
        assert_eq!(night_phase(rise, set, rise), 1.0);
        assert!(night_phase(rise + 30.0, set, rise) < 0.3);
        assert_eq!(night_phase(rise + 45.0, set, rise), 0.0);
    }

    #[test]
    fn warmth_cuts_blue_and_tints_black() {
        assert_eq!(warm((255, 255, 255), 0.0), (255, 255, 255));
        let w = warm((255, 255, 255), 1.0);
        assert!(w.0 == 255 && w.1 < 255 && w.2 < 160, "{w:?}");
        let b = warm((11, 11, 13), 1.0);
        assert!(b.0 > b.2 + 10, "black turns amber: {b:?}");
    }
}
