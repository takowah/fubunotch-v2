// "Watch in island": the browser extension (extension/ytm/watch.js) moves a
// YouTube video into Chrome's Document Picture-in-Picture window; this docks
// that window just under the island and shows or hides it with the island.
// Chrome keeps playing while it is hidden. Win32 only: nothing is drawn here.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_TOPMOST,
};

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
        if search.want.iter().any(|w| text.contains(w.as_str())) && is_chrome(hwnd) {
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
    ok && String::from_utf16_lossy(&name[..len as usize]).to_lowercase().ends_with("chrome.exe")
}

pub fn alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

pub fn rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    r
}

/// Puts the window with its top centre at (`x`, `y`) (physical pixels),
/// right below the island in the z-order, and shows or hides it. Hidden,
/// Chrome keeps playing.
pub fn place(hwnd: HWND, island: Option<HWND>, x: i32, y: i32, shown: bool) {
    unsafe {
        if shown {
            let r = rect(hwnd);
            let w = r.right - r.left;
            let _ = SetWindowPos(hwnd, island, x - w / 2, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            if !IsWindowVisible(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        } else if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
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
