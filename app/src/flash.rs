// Apps that send no Windows notification (WeChat): a new message makes their
// taskbar button flash, and the shell tells every window registered with
// RegisterShellHookWindow (HSHELL_FLASH). A hidden window on a thread of its
// own listens; nothing is polled. As Mini Island's "Aplikasi tanpa
// notifikasi Windows" (Poll-Flashes): the button keeps flashing while the
// message is unread, so a flash counts as a new message only after a quiet
// spell, and one app pops up at most once per cooldown.

use std::collections::HashMap;
use std::sync::OnceLock;

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, RegisterClassW, RegisterShellHookWindow, RegisterWindowMessageW,
    TranslateMessage, MSG, WINDOW_STYLE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// The shell's code for a flashing taskbar button (HSHELL_REDRAW | HSHELL_HIGHBIT).
const HSHELL_FLASH: usize = 0x8006;
/// A flash after this long without one is a new message (ms).
pub const QUIET_MS: u64 = 5_000;
/// One app pops up at most once in this long (ms).
pub const COOLDOWN_MS: u64 = 8_000;

/// Process names (without ".exe", any case) → the app's name on the island.
const APPS: [(&str, &str); 2] = [("weixin", "WeChat"), ("wechat", "WeChat")];

/// A new message in one of those apps.
#[derive(Clone, Debug, PartialEq)]
pub struct Flash {
    pub app: String,
    /// The app's exe ("" when Windows would not say).
    pub exe: String,
}

/// The app an exe belongs to, when it is one FubuNotch v2 listens for.
pub fn app_for(exe: &str) -> Option<&'static str> {
    let file = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    APPS.iter().find(|(p, _)| *p == stem).map(|(_, name)| *name)
}

/// Turns a stream of flashes into "new message" moments.
#[derive(Default)]
pub struct Gate {
    last_flash: HashMap<String, u64>,
    last_shown: HashMap<String, u64>,
}

impl Gate {
    /// Whether `app`'s flash at `now` (ms) is a new message to show.
    pub fn pass(&mut self, app: &str, now: u64) -> bool {
        let before = self.last_flash.insert(app.to_string(), now);
        if before.is_some_and(|t| now.saturating_sub(t) < QUIET_MS) {
            return false;
        }
        if self.last_shown.get(app).is_some_and(|t| now.saturating_sub(*t) < COOLDOWN_MS) {
            return false;
        }
        self.last_shown.insert(app.to_string(), now);
        true
    }
}

/// The key a flash app's logo is kept under (notif's AUMID slot).
pub fn key(exe: &str, app: &str) -> String {
    format!("{}{}", crate::notif::FLASH_PREFIX, if exe.is_empty() { app } else { exe })
}

/// WeChat's exe where its installer puts it, if it is there (the demo's icon).
pub fn installed_wechat() -> Option<String> {
    let roots = ["ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|v| std::env::var(v).ok()).collect::<Vec<_>>();
    roots
        .iter()
        .flat_map(|r| [format!(r"{r}\Tencent\Weixin\Weixin.exe"), format!(r"{r}\Tencent\WeChat\WeChat.exe")])
        .find(|p| std::path::Path::new(p).is_file())
}

type Sink = Box<dyn Fn(Flash) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();
static SHELL_MSG: OnceLock<u32> = OnceLock::new();
static GATE: std::sync::Mutex<Option<Gate>> = std::sync::Mutex::new(None);
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn exe_of(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    let mut name = [0u16; 1024];
    let mut len = name.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(name.as_mut_ptr()), &mut len) }.is_ok();
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(process);
    }
    if ok {
        String::from_utf16_lossy(&name[..len as usize])
    } else {
        String::new()
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if SHELL_MSG.get() == Some(&msg) && wparam.0 == HSHELL_FLASH {
        let exe = exe_of(HWND(lparam.0 as *mut core::ffi::c_void));
        if let Some(app) = app_for(&exe) {
            let now = STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64;
            let pass = GATE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Gate::default).pass(app, now);
            if pass {
                // The app's own icon, read here (COM is on) once per exe.
                let k = key(&exe, app);
                if !exe.is_empty() && crate::notif::logo(&k).is_none() {
                    crate::notif::keep_file_logo(&k, &exe);
                }
                if let Some(sink) = SINK.get() {
                    sink(Flash { app: app.to_string(), exe });
                }
            }
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Listens for flashing taskbar buttons for the app's life; `sink` gets each
/// new message of the apps above (on the listening thread).
pub fn start(sink: impl Fn(Flash) + Send + Sync + 'static) {
    if SINK.set(Box::new(sink)).is_err() {
        return;
    }
    STARTED.get_or_init(std::time::Instant::now);
    std::thread::Builder::new()
        .name("fubunotch-flash".into())
        .spawn(|| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(instance) = GetModuleHandleW(None) else { return };
            let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: w!("FubuNotchFlashWatch"), ..Default::default() };
            RegisterClassW(&class);
            let _ = SHELL_MSG.set(RegisterWindowMessageW(w!("SHELLHOOK")));
            // A hidden top-level window: the shell hook doesn't reach message-only ones.
            let Ok(hwnd) = CreateWindowExW(WS_EX_TOOLWINDOW, w!("FubuNotchFlashWatch"), w!(""), WINDOW_STYLE(WS_POPUP.0), 0, 0, 0, 0, None, None, Some(instance.into()), None) else {
                crate::store::log("flash: no window");
                return;
            };
            if !RegisterShellHookWindow(hwnd).as_bool() {
                crate::store::log("flash: shell hook refused");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wechat_by_its_exe() {
        assert_eq!(app_for(r"C:\Program Files\Tencent\Weixin\Weixin.exe"), Some("WeChat"));
        assert_eq!(app_for(r"C:\Program Files (x86)\Tencent\WeChat\WeChat.exe"), Some("WeChat"));
        assert_eq!(app_for(r"C:\Windows\explorer.exe"), None);
        assert_eq!(app_for(""), None);
    }

    #[test]
    fn a_new_message_after_a_quiet_spell_once_per_cooldown() {
        let mut g = Gate::default();
        assert!(g.pass("WeChat", 0));
        // Still flashing for the same unread message.
        assert!(!g.pass("WeChat", 1_000));
        assert!(!g.pass("WeChat", 4_000));
        // Quiet for 6 s, but inside the cooldown of the last pop-up? 4 s + 6 s = 10 s > 8 s: shown.
        assert!(g.pass("WeChat", 10_000));
        // Quiet long enough but too soon after the last pop-up.
        let mut h = Gate::default();
        assert!(h.pass("WeChat", 0));
        h.last_flash.insert("WeChat".into(), 0);
        assert!(!h.pass("WeChat", 6_000), "cooldown");
        assert!(h.pass("WeChat", 20_000));
    }

    #[test]
    fn its_logo_key() {
        assert_eq!(key(r"C:\W\Weixin.exe", "WeChat"), r"flash:C:\W\Weixin.exe");
        assert_eq!(key("", "WeChat"), "flash:WeChat");
    }
}
