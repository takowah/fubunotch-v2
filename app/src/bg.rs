// A picture behind the island (Settings → Appearance): the user picks a
// file, a copy goes into the data folder, and the island gets it cropped to
// cover its size, the theme's colour laid over it (so text stays readable)
// and the corners cut round. The software renderer doesn't clip images to
// rounded shapes, so the corners are cut here, in the pixels.

use std::path::{Path, PathBuf};

use windows::core::HSTRING;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
use windows::Storage::{FileAccessMode, StorageFile};

/// Asks for a picture with Windows' own dialog. Blocks: call on a thread.
pub fn pick() -> Option<PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<PathBuf> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let name = HSTRING::from("Pictures");
            let spec = HSTRING::from("*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp");
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: windows::core::PCWSTR(name.as_ptr()), pszSpec: windows::core::PCWSTR(spec.as_ptr()) }])?;
            dialog.SetTitle(&HSTRING::from("A picture for the island"))?;
            dialog.Show(None)?;
            let item = dialog.GetResult()?;
            let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let s = path.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
            Ok(PathBuf::from(s))
        })();
        if com.is_ok() {
            CoUninitialize();
        }
        result.ok()
    }
}

/// Copies the picture into `dir` as background.<ext> (the old one goes) and
/// returns the copy's path.
pub fn import(from: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    let ext = from.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    std::fs::create_dir_all(dir)?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("background.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let to = dir.join(format!("background.{ext}"));
    std::fs::copy(from, &to)?;
    Ok(to)
}

/// The picture at `w` x `h` pixels, cropped to cover: RGBA.
fn cover(path: &Path, w: u32, h: u32) -> windows::core::Result<Vec<u8>> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.get()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let (iw, ih) = (decoder.OrientedPixelWidth()? as f32, decoder.OrientedPixelHeight()? as f32);
    let scale = (w as f32 / iw).max(h as f32 / ih);
    let (sw, sh) = ((iw * scale).ceil().max(w as f32) as u32, (ih * scale).ceil().max(h as f32) as u32);
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(sw)?;
    transform.SetScaledHeight(sh)?;
    transform.SetBounds(BitmapBounds { X: (sw - w) / 2, Y: (sh - h) / 2, Width: w, Height: h })?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    Ok(pixels.DetachPixelData()?.to_vec())
}

/// The island's background at `w` x `h` physical pixels: covered, the
/// theme's `tint` laid over at `dim` (0..1), corners cut at `radius`. RGBA
/// and its size, so it can be made on a thread.
pub fn render_rgba(path: &Path, w: u32, h: u32, radius: f32, dim: f32, tint: (u8, u8, u8)) -> Option<(Vec<u8>, u32, u32)> {
    if w == 0 || h == 0 {
        return None;
    }
    let mut px = cover(path, w, h).ok()?;
    if px.len() != (w * h * 4) as usize {
        return None;
    }
    let dim = dim.clamp(0.0, 0.95);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let a = corner_alpha(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32, radius);
            let t = [tint.0, tint.1, tint.2];
            for k in 0..3 {
                let c = px[i + k] as f32 * (1.0 - dim) + t[k] as f32 * dim;
                px[i + k] = c.round() as u8;
            }
            px[i + 3] = (px[i + 3] as f32 * a).round() as u8;
        }
    }
    Some((px, w, h))
}

pub fn image(px: &[u8], w: u32, h: u32) -> slint::Image {
    slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(px, w, h))
}

/// 1 inside the rounded rectangle, 0 outside, a soft pixel at the edge.
fn corner_alpha(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w / 2.0).min(h / 2.0);
    let cx = x.clamp(r, w - r);
    let cy = y.clamp(r, h - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    (r - d + 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_cut_round() {
        assert_eq!(corner_alpha(50.0, 50.0, 100.0, 100.0, 20.0), 1.0, "the middle");
        assert_eq!(corner_alpha(0.5, 0.5, 100.0, 100.0, 20.0), 0.0, "the very corner");
        assert_eq!(corner_alpha(0.5, 50.0, 100.0, 100.0, 20.0), 1.0, "an edge, not a corner");
        let edge = corner_alpha(20.0 - 14.14, 20.0 - 14.14, 100.0, 100.0, 20.0);
        assert!(edge > 0.0 && edge < 1.0, "soft on the arc: {edge}");
    }
}
