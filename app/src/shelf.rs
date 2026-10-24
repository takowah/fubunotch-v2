// The Shelf module: files dropped on the island, kept by path (never copied),
// opened with a click, or put on the clipboard to paste into a chat or a
// folder. Windows' file drops reach us as winit events (Slint has no file
// drop of its own): `DropWatch` takes them before Slint and queues them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use slint::winit_030::{winit, CustomApplicationHandler, EventResult};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::UI::Shell::DROPFILES;

pub const SHELF_MAX: usize = 30;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub path: String,
}

impl Item {
    pub fn name(&self) -> String {
        Path::new(&self.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.clone())
    }

    /// "2.4 MB", "Folder", or "Missing" when the file has gone.
    pub fn detail(&self) -> String {
        match std::fs::metadata(&self.path) {
            Ok(m) if m.is_dir() => "Folder".into(),
            Ok(m) => size(m.len()),
            Err(_) => "Missing".into(),
        }
    }
}

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// Adds `path` at the top (once), keeping at most SHELF_MAX.
pub fn add(items: &mut Vec<Item>, path: &Path) {
    let p = path.display().to_string();
    items.retain(|i| !i.path.eq_ignore_ascii_case(&p));
    items.insert(0, Item { path: p });
    items.truncate(SHELF_MAX);
}

// ── Drops ─────────────────────────────────────────────────────────────────────

pub enum Drop {
    /// A file is being dragged over the island.
    Hover,
    Cancel,
    File(PathBuf),
}

pub static QUEUE: Mutex<Vec<Drop>> = Mutex::new(Vec::new());

/// Catches Windows' file drops on the island window.
pub struct DropWatch {
    pub notify: Box<dyn Fn()>,
}

impl CustomApplicationHandler for DropWatch {
    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        _winit_window: Option<&winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &winit::event::WindowEvent,
    ) -> EventResult {
        use winit::event::WindowEvent;
        let drop = match event {
            WindowEvent::HoveredFile(_) => Drop::Hover,
            WindowEvent::HoveredFileCancelled => Drop::Cancel,
            WindowEvent::DroppedFile(path) => Drop::File(path.clone()),
            _ => return EventResult::Propagate,
        };
        QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(drop);
        (self.notify)();
        EventResult::Propagate
    }
}

// ── Opening and copying ───────────────────────────────────────────────────────

/// Opens the file with its app (a folder in Explorer).
pub fn open(path: &str) {
    if Path::new(path).exists() {
        let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    }
}

/// Puts the file on the clipboard as a file (CF_HDROP): paste it anywhere.
pub fn copy(path: &str) -> bool {
    let wide: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let header = std::mem::size_of::<DROPFILES>();
    unsafe {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, header + wide.len() * 2) else { return false };
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            return false;
        }
        let files = ptr as *mut DROPFILES;
        (*files).pFiles = header as u32;
        (*files).fWide = true.into();
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(header), wide.len() * 2);
        let _ = GlobalUnlock(mem);
        if OpenClipboard(None).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(mem.0))).is_ok();
        let _ = CloseClipboard();
        let _: HGLOBAL = mem;
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2_516_582), "2.4 MB");
    }

    #[test]
    fn the_shelf_keeps_each_file_once_newest_first() {
        let mut items = Vec::new();
        add(&mut items, Path::new("C:\\a.txt"));
        add(&mut items, Path::new("C:\\b.png"));
        add(&mut items, Path::new("c:\\A.TXT"));
        assert_eq!(items.iter().map(|i| i.name()).collect::<Vec<_>>(), ["A.TXT", "b.png"]);
        for i in 0..40 {
            add(&mut items, Path::new(&format!("C:\\f{i}")));
        }
        assert_eq!(items.len(), SHELF_MAX);
    }

    #[test]
    fn a_gone_file_says_so() {
        assert_eq!(Item { path: "C:\\definitely\\not\\here.txt".into() }.detail(), "Missing");
    }
}
