// The themes: a hololive member's colours for every window (the pixel mascot
// follows the same name, see scripts/sprites.py). Settings → Appearance.

use slint::Color;

pub struct Palette {
    pub body: u32,
    pub body_lo: u32,
    pub body_hi: u32,
    pub ink: u32,
    pub dim: u32,
    pub glow: u32,
    pub frost: u32,
    pub deep: u32,
}

/// The palette of `theme` (one of island_core::settings::THEMES).
pub fn palette(theme: &str) -> Palette {
    match theme {
        // Mio: black and crimson, a warm gold eye.
        "mio" => Palette { body: 0x160b10, body_lo: 0x10070b, body_hi: 0x2a1219, ink: 0xfff0f2, dim: 0xb49aa2, glow: 0xff5a6e, frost: 0xffc2ca, deep: 0xb3203a },
        // Towa: night purple and devil pink.
        "towa" => Palette { body: 0x120c1f, body_lo: 0x0c0816, body_hi: 0x221636, ink: 0xf6eeff, dim: 0xa89bbd, glow: 0xc08cff, frost: 0xf0c8ff, deep: 0x6b3fd6 },
        // AZKi: deep wine and her pink.
        "azki" => Palette { body: 0x170c16, body_lo: 0x10080f, body_hi: 0x2a1427, ink: 0xfff0f6, dim: 0xb79aab, glow: 0xff5d9e, frost: 0xffc6dc, deep: 0xb02a64 },
        // Suisei: midnight blue and comet blue.
        "suisei" => Palette { body: 0x0a1030, body_lo: 0x070b22, body_hi: 0x142050, ink: 0xeef3ff, dim: 0x98a6c8, glow: 0x6aa0ff, frost: 0xc9dbff, deep: 0x2f55d8 },
        // Fubuki icy: deep navy, ice white, cyan.
        _ => Palette { body: 0x0b1424, body_lo: 0x091020, body_hi: 0x12203a, ink: 0xe8f6ff, dim: 0x8fa7bd, glow: 0x5fd4ff, frost: 0xb8e8ff, deep: 0x3d74e0 },
    }
}

pub fn rgb(hex: u32) -> Color {
    Color::from_rgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `a` moved toward `b` by `t` (0..1).
pub fn mix(a: u32, b: u32, t: f32) -> Color {
    let ch = |h: u32, s: u32| ((h >> s) & 0xff) as f32;
    let m = |s: u32| (ch(a, s) + (ch(b, s) - ch(a, s)) * t).round() as u8;
    Color::from_rgb_u8(m(16), m(8), m(0))
}

/// Sets one window's colours. A macro: each window type has its own Ice.
#[macro_export]
macro_rules! apply_theme {
    ($ice:expr, $theme:expr) => {{
        let p = $crate::theme::palette($theme);
        let ice = $ice;
        ice.set_theme($theme.into());
        ice.set_body($crate::theme::rgb(p.body));
        ice.set_body_lo($crate::theme::rgb(p.body_lo));
        ice.set_body_hi($crate::theme::rgb(p.body_hi));
        // The text takes the member's colour too: ink toward her pale tone,
        // dim toward her accent.
        ice.set_ink($crate::theme::mix(p.ink, p.frost, 0.3));
        ice.set_dim($crate::theme::mix(p.dim, p.glow, 0.25));
        ice.set_glow($crate::theme::rgb(p.glow));
        ice.set_frost($crate::theme::rgb(p.frost));
        ice.set_deep($crate::theme::rgb(p.deep));
    }};
}
