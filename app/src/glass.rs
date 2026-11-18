// Liquid Glass on Windows: what is behind the island, grabbed from the
// screen and turned into the island's body (island_core::glass does the
// pixels). The way CSS backdrop-filter works, done by hand:
//
// - The island's window is left out of screen capture
//   (SetWindowDisplayAffinity WDA_EXCLUDEFROMCAPTURE, Windows 10 2004+), so
//   grabbing the screen under it finds what is behind it, not itself. That
//   also hides it from screenshots and recordings, so it is on only while
//   the Liquid Glass look is.
// - The screen under the island (plus a margin for the lens) is grabbed
//   already shrunk to a quarter (GDI StretchBlt, HALFTONE averages).
// - A worker thread blurs, bends and cuts it, and hands the RGBA back. It
//   sleeps on its channel when there is nothing to do; the app asks ~10
//   times a second only while the island is shown, and an unchanged screen
//   (same hash) costs no redraw.
//
// Snapshots have no screen: KANMURI_DEMO_BACKDROP=<picture> (else the
// snapshot's own gradient) stands in for it.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use island_core::glass::{self, Glass, Shape};

/// Physical px per captured px.
pub const FACTOR: usize = 4;

/// One frame to make: the island's rectangle in physical screen pixels.
pub struct Job {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub radius: f32,
    pub scale: f32,
    pub was_light: bool,
    /// The last frame's hash and shape key: the same again = nothing to do.
    pub last_hash: u64,
    pub last_key: String,
    pub key: String,
}

pub struct Done {
    pub key: String,
    pub hash: u64,
    pub light: bool,
    /// None: the screen under the island hasn't changed.
    pub pixels: Option<(Vec<u8>, u32, u32)>,
}

type Reply = Box<dyn FnOnce(Done) + Send>;

/// Leaves the island out of screen capture (on) or puts it back (off).
/// False if Windows refused (older than 10 2004): no glass then, or the
/// island would capture itself.
pub fn exclude_from_capture(hwnd: windows::Win32::Foundation::HWND, on: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    unsafe { SetWindowDisplayAffinity(hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE }).is_ok() }
}

/// Hands `job` to the worker; `reply` runs on the worker with the result.
pub fn request(job: Job, reply: Reply) {
    static WORKER: OnceLock<Mutex<Sender<(Job, Reply)>>> = OnceLock::new();
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = channel::<(Job, Reply)>();
        std::thread::Builder::new()
            .name("glass".into())
            .spawn(move || {
                let mut g = Glass::new();
                let mut grab = Grabber::default();
                let mut out = Vec::new();
                // Blocks (no CPU) while nothing is asked.
                while let Ok((job, reply)) = rx.recv() {
                    reply(live(&mut g, &mut grab, &mut out, &job));
                }
            })
            .expect("glass thread");
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send((job, reply));
}

fn margin(scale: f32) -> i32 {
    (glass::MARGIN * scale).ceil() as i32
}

fn live(g: &mut Glass, grab: &mut Grabber, out: &mut Vec<u8>, job: &Job) -> Done {
    let unchanged = Done { key: job.key.clone(), hash: job.last_hash, light: job.was_light, pixels: None };
    let m = margin(job.scale);
    let Some((px, lw, lh, cx, cy, cw, _ch)) = grab.grab(job.x - m, job.y - m, job.w as i32 + 2 * m, job.h as i32 + 2 * m) else {
        return unchanged;
    };
    let hash = g.load(px, lw, lh, true);
    if hash == job.last_hash && job.key == job.last_key {
        return unchanged;
    }
    let factor = cw as f32 / lw as f32;
    let shape = Shape { w: job.w as usize, h: job.h as usize, radius: job.radius, scale: job.scale, ox: (job.x - cx) as f32, oy: (job.y - cy) as f32, factor };
    out.resize(shape.w * shape.h * 4, 0);
    let light = g.render(&shape, job.was_light, out);
    Done { key: job.key.clone(), hash, light, pixels: Some((out.clone(), job.w, job.h)) }
}

/// The screen, grabbed small. Keeps its DCs and bitmap between frames.
#[derive(Default)]
struct Grabber {
    mem: isize,
    bmp: isize,
    bits: usize,
    size: (i32, i32),
}

impl Grabber {
    /// The screen's (x, y, w, h) clamped to the desktop, shrunk by FACTOR:
    /// BGRA pixels, their size, and the rectangle actually grabbed.
    #[allow(clippy::type_complexity)]
    fn grab(&mut self, x: i32, y: i32, w: i32, h: i32) -> Option<(&[u8], usize, usize, i32, i32, i32, i32)> {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
        unsafe {
            let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            let (x0, y0) = (x.max(vx), y.max(vy));
            let (x1, y1) = ((x + w).min(vx + vw), (y + h).min(vy + vh));
            let (cw, ch) = (x1 - x0, y1 - y0);
            if cw < FACTOR as i32 || ch < FACTOR as i32 {
                return None;
            }
            let (lw, lh) = ((cw as usize).div_ceil(FACTOR) as i32, (ch as usize).div_ceil(FACTOR) as i32);
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            if self.size != (lw, lh) || self.mem == 0 {
                self.free();
                let mem = CreateCompatibleDC(Some(screen));
                let mut info = BITMAPINFO::default();
                info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = lw;
                // Top-down rows.
                info.bmiHeader.biHeight = -lh;
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB.0;
                let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
                let Ok(bmp) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                    let _ = DeleteDC(mem);
                    ReleaseDC(None, screen);
                    return None;
                };
                SelectObject(mem, bmp.into());
                SetStretchBltMode(mem, HALFTONE);
                let _ = SetBrushOrgEx(mem, 0, 0, None);
                self.mem = mem.0 as isize;
                self.bmp = bmp.0 as isize;
                self.bits = bits as usize;
                self.size = (lw, lh);
            }
            let mem = HDC(self.mem as *mut core::ffi::c_void);
            let ok = StretchBlt(mem, 0, 0, lw, lh, Some(screen), x0, y0, cw, ch, SRCCOPY).as_bool();
            ReleaseDC(None, screen);
            let _ = GdiFlush();
            if !ok || self.bits == 0 {
                return None;
            }
            let px = std::slice::from_raw_parts(self.bits as *const u8, (lw * lh * 4) as usize);
            Some((px, lw as usize, lh as usize, x0, y0, cw, ch))
        }
    }

    fn free(&mut self) {
        use windows::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, HBITMAP, HDC};
        unsafe {
            if self.mem != 0 {
                let _ = DeleteDC(HDC(self.mem as *mut core::ffi::c_void));
            }
            if self.bmp != 0 {
                let _ = DeleteObject(HBITMAP(self.bmp as *mut core::ffi::c_void).into());
            }
        }
        *self = Grabber::default();
    }
}

// ── Snapshots ───────────────────────────────────────────────────────────────

/// What is "behind" a snapshot's window of `w` x `h` physical px, RGBA:
/// KANMURI_DEMO_BACKDROP's picture (cropped to cover), else a desktop-ish
/// gradient. Decoded once per size.
pub fn demo_backdrop(w: u32, h: u32) -> Vec<u8> {
    static CACHE: Mutex<Option<((u32, u32), Vec<u8>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((size, px)) = cache.as_ref() {
        if *size == (w, h) {
            return px.clone();
        }
    }
    let picture = std::env::var("KANMURI_DEMO_BACKDROP")
        .ok()
        .filter(|p| !p.is_empty())
        // Windows' picture decoder wants backslashes and a full path.
        .map(|p| std::path::absolute(p.replace('/', "\\")).unwrap_or_default())
        .and_then(|p| crate::bg::cover(&p, w, h).ok())
        .filter(|px| px.len() == (w * h * 4) as usize);
    let px = picture.unwrap_or_else(|| {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..w * h {
            let y = (i / w) as f32 / h as f32;
            px.extend_from_slice(&[(52.0 + 40.0 * y) as u8, (58.0 + 30.0 * y) as u8, (74.0 + 20.0 * y) as u8, 255]);
        }
        px
    });
    *cache = Some(((w, h), px.clone()));
    px
}

/// A snapshot's glass: the island at (x, y, w, h) physical px in a window
/// of `win` physical px, over the demo backdrop. RGBA and whether it is light.
pub fn demo(win: (u32, u32), x: i32, y: i32, w: u32, h: u32, radius: f32, scale: f32, was_light: bool) -> Option<(Vec<u8>, bool)> {
    let back = demo_backdrop(win.0, win.1);
    let m = margin(scale);
    let (x0, y0) = ((x - m).max(0), (y - m).max(0));
    let (x1, y1) = ((x + w as i32 + m).min(win.0 as i32), (y + h as i32 + m).min(win.1 as i32));
    let (cw, ch) = ((x1 - x0) as usize, (y1 - y0) as usize);
    if cw < FACTOR || ch < FACTOR {
        return None;
    }
    let mut crop = Vec::with_capacity(cw * ch * 4);
    for row in y0 as usize..y1 as usize {
        let i = (row * win.0 as usize + x0 as usize) * 4;
        crop.extend_from_slice(&back[i..i + cw * 4]);
    }
    let (small, lw, lh) = glass::downscale(&crop, cw, ch, FACTOR);
    let mut g = Glass::new();
    g.load(&small, lw, lh, false);
    let shape = Shape { w: w as usize, h: h as usize, radius, scale, ox: (x - x0) as f32, oy: (y - y0) as f32, factor: FACTOR as f32 };
    let mut out = vec![0u8; w as usize * h as usize * 4];
    let light = g.render(&shape, was_light, &mut out);
    Some((out, light))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a desktop (not a locked PC or CI): `cargo test -p kanmuri grab -- --ignored`.
    #[test]
    #[ignore]
    fn grab_reads_the_screen_small() {
        let mut g = Grabber::default();
        let (px, lw, lh, x, y, cw, ch) = g.grab(-50, -50, 450, 250).map(|(p, a, b, c, d, e, f)| (p.to_vec(), a, b, c, d, e, f)).expect("grab");
        assert!(x >= -50 && y >= -50 && cw <= 450 && ch <= 250);
        assert_eq!((lw, lh), ((cw as usize).div_ceil(FACTOR), (ch as usize).div_ceil(FACTOR)));
        assert_eq!(px.len(), lw * lh * 4);
        // Again at the same size reuses the bitmap.
        let again = g.grab(-50, -50, 450, 250).map(|(p, ..)| p.len());
        assert_eq!(again, Some(px.len()));
        let mut glass = Glass::new();
        glass.load(&px, lw, lh, true);
        let shape = Shape { w: 200, h: 60, radius: 30.0, scale: 1.0, ox: 20.0, oy: 20.0, factor: cw as f32 / lw as f32 };
        let mut out = vec![0u8; 200 * 60 * 4];
        glass.render(&shape, false, &mut out);
        assert_eq!(out[3], 0);
        assert_eq!(out[(30 * 200 + 100) * 4 + 3], 255);
    }
}
