// "Watch in island": a browser's picture-in-picture window docked just under
// the island, shown or hidden with it (the browser keeps playing while it is
// hidden). Two kinds: any video PiP Chrome or Edge opens (YouTube's own
// button, a right-click, Chrome's media controls), found as it appears
// through a WinEvent hook; and the Document PiP the extension's player button
// opens (extension/ytm/watch.js), found by the title it reports. Win32 only:
// nothing is drawn here.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    GetSystemMetrics, PostMessageW, SetWindowPos, SM_CXSCREEN, SM_CYSCREEN, SM_XVIRTUALSCREEN, SWP_NOZORDER, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_NAMECHANGE, GWL_EXSTYLE, OBJID_WINDOW, SWP_NOACTIVATE,
    SWP_NOSIZE, SET_WINDOW_POS_FLAGS, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_CLOSE, WS_EX_TOPMOST,
};

/// The titles Chrome and Edge give their video picture-in-picture window.
const PIP_TITLES: [&str; 2] = ["Picture in picture", "Picture-in-picture"];

/// A topmost browser window (Chrome's window class) of chrome.exe or msedge.exe.
fn browser_topmost(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    if String::from_utf16_lossy(&class[..n]) != "Chrome_WidgetWin_1" {
        return false;
    }
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    ex & WS_EX_TOPMOST.0 != 0 && is_chrome(hwnd)
}

fn title(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
    String::from_utf16_lossy(&text[..n])
}

/// Whether `hwnd` is a browser's video picture-in-picture window on screen.
pub fn is_plain_pip(hwnd: HWND) -> bool {
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    visible && PIP_TITLES.iter().any(|t| title(hwnd).eq_ignore_ascii_case(t)) && browser_topmost(hwnd)
}

/// A browser's video picture-in-picture window on screen, if one is open
/// (also one opened before FubuNotch started). Chrome keeps hidden ones of
/// its own around: only a visible one counts.
pub fn find_plain() -> Option<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
        if is_plain_pip(hwnd) {
            *found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut Option<HWND> as isize));
    }
    found
}

/// Closes a video picture-in-picture window: the browser puts the video back
/// in its tab.
pub fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, Default::default(), Default::default());
    }
}

static ON_CHANGE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 || child != 0 || hwnd.0.is_null() {
        return;
    }
    // Shown or renamed: a browser's PiP may have opened. Hidden or destroyed:
    // only the docked one matters (Chrome hides a closed PiP).
    let worth = if event == EVENT_OBJECT_DESTROY || event == EVENT_OBJECT_HIDE {
        hwnd.0 as isize == TRACKED.load(std::sync::atomic::Ordering::Relaxed)
    } else {
        is_plain_pip(hwnd)
    };
    if worth {
        if let Some(f) = ON_CHANGE.get() {
            f();
        }
    }
}

/// Calls `changed` whenever a browser's picture-in-picture window may have
/// appeared or a window went away: Windows reports it, nothing is polled.
/// Install once, on the UI thread (its message loop delivers the events).
pub fn hook(changed: impl Fn() + Send + Sync + 'static) {
    if ON_CHANGE.set(Box::new(changed)).is_err() {
        return;
    }
    unsafe {
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        SetWinEventHook(EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, None, Some(on_event), 0, 0, flags);
        SetWinEventHook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, None, Some(on_event), 0, 0, flags);
    }
}

/// Chrome's picture-in-picture window whose title holds `window` (the
/// extension's marker) or `title` (the video's): a topmost Chrome window.
pub fn find(window: &str, title: &str) -> Option<HWND> {
    struct Search {
        want: Vec<String>,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut class = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if String::from_utf16_lossy(&class[..n]) != "Chrome_WidgetWin_1" {
            return true.into();
        }
        let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        if ex & WS_EX_TOPMOST.0 == 0 {
            return true.into();
        }
        let mut text = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, &mut text) } as usize;
        let text = String::from_utf16_lossy(&text[..n]);
        // Visible only: Chrome keeps closed PiP windows around, hidden.
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        if visible && search.want.iter().any(|w| text.contains(w.as_str())) && is_chrome(hwnd) {
            search.found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let want: Vec<String> = [window, title].iter().map(|s| s.trim().to_string()).filter(|s| s.chars().count() >= 4).collect();
    if want.is_empty() {
        return None;
    }
    let mut search = Search { want, found: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

fn is_chrome(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(process) = (unsafe {
        windows::Win32::System::Threading::OpenProcess(windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
    }) else {
        return false;
    };
    let mut name = [0u16; 260];
    let mut len = name.len() as u32;
    let ok = unsafe {
        windows::Win32::System::Threading::QueryFullProcessImageNameW(
            process,
            windows::Win32::System::Threading::PROCESS_NAME_WIN32,
            windows::core::PWSTR(name.as_mut_ptr()),
            &mut len,
        )
    }
    .is_ok();
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(process);
    }
    let path = String::from_utf16_lossy(&name[..len as usize]).to_lowercase();
    ok && (path.ends_with("\\chrome.exe") || path.ends_with("\\msedge.exe"))
}

/// Whether the window is still an open picture-in-picture. Chrome doesn't
/// destroy a closed one, it hides it (and may show it again for the next
/// video): hidden = closed. FubuNotch never hides it itself for that reason,
/// it moves it out of sight (`place`).
pub fn alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() && IsWindowVisible(hwnd).as_bool() }
}

/// The docked window, for the hook to report its end (0 = none).
static TRACKED: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub fn track(hwnd: Option<HWND>) {
    TRACKED.store(hwnd.map_or(0, |h| h.0 as isize), std::sync::atomic::Ordering::Relaxed);
}

pub fn rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    r
}

/// Puts the window with its top centre at (`x`, `y`) (physical pixels),
/// right below the island in the z-order, or (not `shown`) out of sight left
/// of every screen. It stays a visible window, so Chrome keeps playing and a
/// close (Chrome hides it) still tells. `width`: the size it gets (Chrome
/// remembers a PiP's last size, which may be anything); the height follows
/// the video's shape, as Chrome keeps it.
pub fn place(hwnd: HWND, island: Option<HWND>, x: i32, y: i32, shown: bool, width: i32) {
    let r = rect(hwnd);
    let (mut w, h) = (r.right - r.left, r.bottom - r.top);
    let mut size = SWP_NOSIZE;
    let mut height = h;
    if w > 0 && (w - width).abs() > 4 {
        height = (width as f64 * h as f64 / w as f64).round() as i32;
        w = width;
        size = SET_WINDOW_POS_FLAGS(0);
    }
    let left = if shown { x - w / 2 } else { (unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) }) - w - 64 };
    if size == SWP_NOSIZE && r.left == left && r.top == y {
        return;
    }
    unsafe {
        let _ = SetWindowPos(hwnd, island, left, y, w, height, size | SWP_NOACTIVATE);
    }
}

/// Lets the window go where Chrome would put it (bottom right of the main
/// screen), in sight: FubuNotch is quitting.
pub fn release(hwnd: HWND) {
    let r = rect(hwnd);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    unsafe {
        let x = GetSystemMetrics(SM_CXSCREEN) - w - 16;
        let y = GetSystemMetrics(SM_CYSCREEN) - h - 64;
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER);
    }
}

/// Whether the pointer is over the window (so the island stays open while
/// you look at the video).
pub fn pointer_over(hwnd: HWND) -> bool {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return false;
    }
    let r = rect(hwnd);
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom && unsafe { IsWindowVisible(hwnd).as_bool() }
}
