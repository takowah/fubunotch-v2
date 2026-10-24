// Checks of the UI without a screen (a locked PC, CI): with
// FUBUNOTCH_V2_SNAPSHOT=<file.png> the app draws into memory instead of
// windows, lets the animations run out, and writes what a window shows to a
// PNG over a desktop-like background. Nothing here runs otherwise.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::PlatformError;

/// Pixels per logical pixel in the PNG: sharp enough to judge.
pub const SCALE: f32 = 1.5;

struct SnapPlatform;

thread_local! {
    static PLATFORM: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

/// The windows made and the clock, where the app can reach them.
struct Shared {
    windows: RefCell<Vec<Rc<MinimalSoftwareWindow>>>,
    now: Cell<Duration>,
}

impl Platform for SnapPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        w.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: SCALE });
        PLATFORM.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                p.windows.borrow_mut().push(w.clone());
            }
        });
        Ok(w)
    }

    fn duration_since_start(&self) -> Duration {
        PLATFORM.with(|p| p.borrow().as_ref().map(|p| p.now.get())).unwrap_or_default()
    }
}

/// The output file, when a snapshot was asked for.
pub fn requested() -> Option<String> {
    std::env::var("FUBUNOTCH_V2_SNAPSHOT").ok().filter(|s| !s.is_empty())
}

/// Puts Slint on the in-memory platform. Call before any window is made.
pub fn install() -> Result<(), PlatformError> {
    PLATFORM.with(|p| *p.borrow_mut() = Some(Rc::new(Shared { windows: RefCell::new(Vec::new()), now: Cell::new(Duration::ZERO) })));
    slint::platform::set_platform(Box::new(SnapPlatform))
        .map_err(|_| PlatformError::Other("a platform is already set".into()))
}

/// Lets `ms` of time pass: timers fire, animations move.
pub fn advance(ms: u64) {
    let step = 16;
    let mut left = ms;
    while left > 0 {
        let d = left.min(step);
        PLATFORM.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                p.now.set(p.now.get() + Duration::from_millis(d));
            }
        });
        slint::platform::update_timers_and_animations();
        left -= d;
    }
}

/// Draws window `index` (0 = the first made: the island) at `size` logical
/// pixels and writes it to `path`.
pub fn write(index: usize, size: (f32, f32), path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let window = PLATFORM
        .with(|p| p.borrow().as_ref().and_then(|p| p.windows.borrow().get(index).cloned()))
        .ok_or("no such window")?;
    let (w, h) = ((size.0 * SCALE).round() as u32, (size.1 * SCALE).round() as u32);
    window.set_size(slint::PhysicalSize::new(w, h));
    advance(50);
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buf, w as usize);
    });
    // Over a desktop-ish backdrop: a gradient, so transparency and glow show.
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for (i, px) in buf.iter().enumerate() {
        let y = (i as u32 / w) as f32 / h as f32;
        let back = [(52.0 + 40.0 * y) as u32, (58.0 + 30.0 * y) as u32, (74.0 + 20.0 * y) as u32];
        let a = px.alpha as u32;
        let c = [px.red as u32, px.green as u32, px.blue as u32];
        for k in 0..3 {
            rgb.push((c[k] + back[k] * (255 - a) / 255).min(255) as u8);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
