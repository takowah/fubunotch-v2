// Settings → Appearance → Look, on the island: the body's colour and how
// solid it is, the ink that reads on it, the night's warmth, the wallpaper's
// colour (read at start and when Windows says the wallpaper changed, never
// polled). Liquid Glass' pixels are made in glass.rs; here it only picks the
// ink and the fallback body (while the glass is not drawn yet).

use island_core::look::{self, Rgb};
use island_core::settings::LookSettings;
use slint::Color;

/// Mini Island's body: the Black look, and dark Liquid Glass.
const BLACK: Rgb = (0x0b, 0x0b, 0x0d);
/// Light Liquid Glass (over a light backdrop).
const PALE: Rgb = (0xf2, 0xf2, 0xf5);
/// How solid each look's body is.
const SOLID: f32 = 0.949;
/// Liquid Glass while its picture isn't drawn (springing to a new size).
/// Windows' own blur (ACCENT_ENABLE_BLURBEHIND) is not used: it ignores the
/// window region and blurred the whole (much bigger) window as a square
/// around the island (seen live 18 Nov 2026).
const LIQUID: f32 = 0.82;

/// What the island draws with: set on its Mini global.
#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub base: Rgb,
    pub solid: f32,
    pub closed: f32,
    pub light: bool,
    pub ink: [Rgb; 5],
    pub wash: Rgb,
    /// Liquid Glass: the island draws the blurred backdrop (glass.rs).
    pub glass: bool,
}

/// The ink and its four greys, on a dark body and on a light one.
const INK_DARK_BODY: [Rgb; 5] = [(0xff, 0xff, 0xff), (0xd2, 0xd2, 0xd7), (0xc2, 0xc2, 0xc7), (0xb4, 0xb4, 0xba), (0xa1, 0xa1, 0xa6)];
const INK_LIGHT_BODY: [Rgb; 5] = [(0x14, 0x14, 0x16), (0x2e, 0x2e, 0x33), (0x3a, 0x3a, 0x40), (0x48, 0x48, 0x4e), (0x5c, 0x5c, 0x63)];

/// The island's colours for `look`, the wallpaper's colour (if known) and
/// the night's warmth now (0..1, already times the user's strength);
/// `backdrop_light`: Liquid Glass sits over something light (dark ink).
pub fn colors(look: &LookSettings, wallpaper: Option<Rgb>, warmth: f32, backdrop_light: bool) -> Colors {
    let style = look.style();
    let (body, solid) = match style {
        "color" => (look.color_rgb(), SOLID),
        "wallpaper" => (wallpaper.unwrap_or(BLACK), SOLID),
        "liquid" => (if backdrop_light { PALE } else { BLACK }, LIQUID),
        _ => (BLACK, SOLID),
    };
    let light = look::wants_dark_ink(body);
    let ink = if light { INK_LIGHT_BODY } else { INK_DARK_BODY };
    Colors {
        base: look::warm(body, warmth),
        solid,
        closed: look.closed_alpha(),
        light,
        ink: ink.map(|c| look::warm(c, warmth * 0.8)),
        wash: if light { (0, 0, 0) } else { (255, 255, 255) },
        glass: style == "liquid",
    }
}

pub fn color(c: Rgb) -> Color {
    Color::from_rgb_u8(c.0, c.1, c.2)
}

/// The warmth now: the user's strength times how far into the night it is.
/// Night runs from `sunset` to `sunrise` (minutes after midnight).
pub fn warmth(look: &LookSettings, now_min: f32, sunset: u32, sunrise: u32) -> f32 {
    if !look.night_warm {
        return 0.0;
    }
    look.night_warmth.min(100) as f32 / 100.0 * look::night_phase(now_min, sunset as f32, sunrise as f32)
}

// ── The wallpaper ────────────────────────────────────────────────────────────

/// The wallpaper's colour, darkened for the island. Blocks (decodes a
/// picture): call on a thread. FUBUNOTCH_V2_DEMO_WALLPAPER=<picture | rrggbb>
/// stands in for the desktop's (snapshots).
pub fn wallpaper_color() -> Option<Rgb> {
    if let Ok(demo) = std::env::var("FUBUNOTCH_V2_DEMO_WALLPAPER") {
        if let Some(c) = look::parse_hex(&demo) {
            return look::wallpaper_color(&[c.0, c.1, c.2, 255]);
        }
        return small_pixels(std::path::Path::new(&demo)).ok().and_then(|px| look::wallpaper_color(&px));
    }
    let path = wallpaper_path();
    if let Some(px) = path.as_deref().and_then(|p| small_pixels(p).ok()) {
        return look::wallpaper_color(&px);
    }
    // No picture: a solid-colour desktop.
    let c = unsafe { windows::Win32::Graphics::Gdi::GetSysColor(windows::Win32::Graphics::Gdi::COLOR_DESKTOP) };
    look::wallpaper_color(&[c as u8, (c >> 8) as u8, (c >> 16) as u8, 255])
}

/// The picture on the desktop: Windows' own setting, else its transcoded copy.
fn wallpaper_path() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut buf = [0u16; 520];
    let ok = unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr().cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    if ok.is_ok() {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let p = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n]));
        if n > 0 && p.is_file() {
            return Some(p);
        }
    }
    let t = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\Themes\TranscodedWallpaper");
    t.is_file().then_some(t)
}

/// The picture scaled down to 48 x 27: plenty for an average. RGBA.
fn small_pixels(path: &std::path::Path) -> windows::core::Result<Vec<u8>> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::{FileAccessMode, StorageFile};
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.get()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(48)?;
    transform.SetScaledHeight(27)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    Ok(pixels.DetachPixelData()?.to_vec())
}
