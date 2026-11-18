// The microphone and camera dots: Windows notes every app that uses the
// microphone or the webcam under HKCU's CapabilityAccessManager\ConsentStore
// (one subkey per Store app, `NonPackaged\<path with # for \>` for the rest);
// while an app uses it, its LastUsedTimeStop is 0. A thread of its own waits
// on RegNotifyChangeKeyValue for both keys (nothing is polled) and reads them
// again only when Windows says they changed. As Mini Island's "Mikrofon &
// kamera" (Check-Sensors).

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, ERROR_SUCCESS, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_QWORD,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};

const STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

/// Who uses the microphone and the camera now (friendly app names).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InUse {
    pub mic: Vec<String>,
    pub cam: Vec<String>,
}

/// An app's entry is "in use" while it started (a time) and has not stopped (0).
pub fn active(start: Option<u64>, stop: Option<u64>) -> bool {
    stop == Some(0) && start.is_some_and(|s| s > 0)
}

/// A ConsentStore subkey as a name people know: an exe path
/// ("C:#Program Files#Zoom#bin#Zoom.exe") → "Zoom"; a Store package
/// ("Microsoft.WindowsCamera_8wekyb3d8bbwe") → "WindowsCamera".
pub fn friendly(key: &str) -> String {
    let name = if key.contains('#') {
        let file = key.rsplit('#').next().unwrap_or(key);
        match file.rfind('.') {
            Some(dot) if dot > 0 => file[..dot].to_string(),
            _ => file.to_string(),
        }
    } else {
        let family = key.split('_').next().unwrap_or(key);
        family.rsplit('.').next().unwrap_or(family).to_string()
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(parent: HKEY, path: &str) -> Option<Key> {
    let mut key = HKEY::default();
    let ok = unsafe { RegOpenKeyExW(parent, &HSTRING::from(path), None, KEY_READ, &mut key) };
    (ok == ERROR_SUCCESS).then_some(Key(key))
}

fn subkeys(key: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let r = unsafe { RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    out
}

fn qword(key: HKEY, sub: &str, value: &str) -> Option<u64> {
    let mut v = 0u64;
    let mut size = 8u32;
    let r = unsafe {
        RegGetValueW(key, &HSTRING::from(sub), &HSTRING::from(value), RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as *mut _), Some(&mut size))
    };
    (r == ERROR_SUCCESS).then_some(v)
}

/// The apps using one capability now, its Store apps and its NonPackaged ones.
fn users(cap: &Key) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    let mut scan = |key: HKEY| {
        for name in subkeys(key) {
            if name == "NonPackaged" {
                continue;
            }
            if active(qword(key, &name, "LastUsedTimeStart"), qword(key, &name, "LastUsedTimeStop")) {
                let f = friendly(&name);
                if !f.is_empty() && !list.contains(&f) {
                    list.push(f);
                }
            }
        }
    };
    scan(cap.0);
    if let Some(np) = open(cap.0, "NonPackaged") {
        scan(np.0);
    }
    list
}

/// Watches both keys on a thread of its own; `sink` gets who uses them now,
/// once at start and then whenever it changes. Runs for the app's life (it
/// costs nothing while it waits).
pub fn start(sink: impl Fn(InUse) + Send + 'static) {
    std::thread::Builder::new()
        .name("kanmuri-sensors".into())
        .spawn(move || run(sink))
        .ok();
}

fn run(sink: impl Fn(InUse)) {
    let Some(store) = open(HKEY_CURRENT_USER, STORE) else {
        crate::store::log("sensors: no ConsentStore key");
        return;
    };
    // Auto-reset events: microphone, webcam, and the store itself (a key made
    // the first time an app uses the camera).
    let events: Vec<HANDLE> = (0..3).filter_map(|_| unsafe { CreateEventW(None, false, false, PCWSTR::null()).ok() }).collect();
    if events.len() != 3 {
        return;
    }
    let mut keys: [Option<Key>; 2] = [None, None];
    let mut last: Option<InUse> = None;
    loop {
        // Armed before reading, so a change in between still wakes the loop.
        for (i, cap) in ["microphone", "webcam"].iter().enumerate() {
            if keys[i].is_none() {
                keys[i] = open(store.0, cap);
            }
            if let Some(k) = &keys[i] {
                unsafe {
                    let _ = RegNotifyChangeKeyValue(k.0, true, REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME, Some(events[i]), true);
                }
            }
        }
        if keys.iter().any(Option::is_none) {
            unsafe {
                let _ = RegNotifyChangeKeyValue(store.0, false, REG_NOTIFY_CHANGE_NAME, Some(events[2]), true);
            }
        }
        let now = InUse {
            mic: keys[0].as_ref().map(users).unwrap_or_default(),
            cam: keys[1].as_ref().map(users).unwrap_or_default(),
        };
        if last.as_ref() != Some(&now) {
            sink(now.clone());
            last = Some(now);
        }
        let woke = unsafe { WaitForMultipleObjects(&events, false, INFINITE) };
        if woke.0 >= WAIT_OBJECT_0.0 + events.len() as u32 {
            // WAIT_FAILED: nothing to wait on any more.
            crate::store::log("sensors: wait failed");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_use_while_started_and_not_stopped() {
        assert!(active(Some(134_000_000_000_000_000), Some(0)));
        assert!(!active(Some(134_000_000_000_000_000), Some(134_000_000_100_000_000)), "stopped");
        assert!(!active(Some(0), Some(0)), "never started");
        assert!(!active(None, Some(0)));
        assert!(!active(Some(5), None));
    }

    #[test]
    fn names_people_know() {
        assert_eq!(friendly("C:#Program Files#Zoom#bin#Zoom.exe"), "Zoom");
        assert_eq!(friendly("C:#Users#me#AppData#Local#Discord#app-1.0.9#Discord.exe"), "Discord");
        assert_eq!(friendly("C:#Program Files (x86)#Google#Chrome#Application#chrome.exe"), "Chrome");
        assert_eq!(friendly("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "WindowsCamera");
        assert_eq!(friendly("MSTeams_8wekyb3d8bbwe"), "MSTeams");
        assert_eq!(friendly(""), "");
    }
}
