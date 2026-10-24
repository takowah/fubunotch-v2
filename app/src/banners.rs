// Notifications only on the island: while the Notifications module is on,
// Windows' own pop-up banners are off app by app, with the switch Windows
// Settings → Notifications → <app> → "Show notification banners" writes
// (ShowBanner = 0 under the app's key). The notifications still land in
// Windows' notification centre, where FubuNotch reads them.
//
// Three layers, so even an app's first notification goes to the island: every
// app in the Start menu up front (by its AUMID, also those that never sent
// one), each app Windows adds a key for later (`watch`), and any other app at
// its first notification. What each app had is kept, to put back exactly; keys
// FubuNotch made are removed again. Per user (HKCU), no admin rights.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, RegQueryInfoKeyW,
    RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_DWORD, REG_NOTIFY_CHANGE_NAME, RRF_RT_REG_DWORD,
};

/// Where Windows keeps each app's notification settings.
pub const SETTINGS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings";
const VALUE: &str = "ShowBanner";

/// What an app had before FubuNotch turned its banners off.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    /// Its ShowBanner; none = no value (Windows then shows banners).
    pub before: Option<u32>,
    /// Its key didn't exist: FubuNotch made it, and removes it again.
    pub created: bool,
}

fn open(path: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, access, &mut key).is_ok().then_some(key) }
}

/// The apps Windows has notification settings for (their keys' names).
pub fn apps(base: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(key) = open(base, KEY_READ) else { return out };
    let mut name = [0u16; 512];
    unsafe {
        for i in 0.. {
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// Every app in the Start menu by its AUMID (as `Get-StartApps` lists them),
/// read again at most every few minutes.
pub fn start_apps() -> Vec<String> {
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, list)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(300) {
            return list.clone();
        }
    }
    let list = read_start_apps();
    *cache = Some((Instant::now(), list.clone()));
    list
}

fn read_start_apps() -> Vec<String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, BHID_EnumItems, KF_FLAG_DEFAULT, SIGDN_PARENTRELATIVEPARSING};
    let mut out = Vec::new();
    unsafe {
        // The UI thread has COM already (then this only counts once more).
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _undo = scopeguard(com.is_ok());
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut got = [None];
            let mut n = 0u32;
            if items.Next(&mut got, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            let Some(item) = got[0].take() else { break };
            if let Ok(name) = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING) {
                let aumid = name.to_string().unwrap_or_default();
                CoTaskMemFree(Some(name.0 as *const _));
                // Web links and bare file paths in the Start menu have no
                // AUMID their notifications would come under.
                if !aumid.is_empty() && !aumid.contains("://") && !aumid.contains(":\\") {
                    out.push(aumid);
                }
            }
        }
    }
    out
}

/// Undoes CoInitializeEx when it took (S_OK or S_FALSE), at the end of scope.
fn scopeguard(initialized: bool) -> impl Drop {
    struct Undo(bool);
    impl Drop for Undo {
        fn drop(&mut self) {
            if self.0 {
                unsafe { windows::Win32::System::Com::CoUninitialize() };
            }
        }
    }
    Undo(initialized)
}

/// The app's ShowBanner, or None when it has none.
pub fn read(base: &str, aumid: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(format!("{base}\\{aumid}")),
            &HSTRING::from(VALUE),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok().then_some(value)
}

fn exists(base: &str, aumid: &str) -> bool {
    open(&format!("{base}\\{aumid}"), KEY_READ).map(|k| unsafe { RegCloseKey(k) }).is_some()
}

/// Sets the app's ShowBanner (making its key if need be), or removes it (None).
pub fn write(base: &str, aumid: &str, value: Option<u32>) -> Result<(), String> {
    let key = HSTRING::from(format!("{base}\\{aumid}"));
    let name = HSTRING::from(VALUE);
    let r = unsafe {
        match value {
            Some(v) => RegSetKeyValueW(HKEY_CURRENT_USER, &key, &name, REG_DWORD.0, Some(&v as *const u32 as *const _), 4),
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name),
        }
    };
    if r.is_ok() || (value.is_none() && r == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(format!("{aumid}: {r:?}"))
    }
}

/// Removes the app's key and, for an AUMID that is a path, the parents made
/// with it: each only while it holds nothing (no values, no keys).
fn remove_empty(base: &str, aumid: &str) {
    let mut parts: Vec<&str> = aumid.split('\\').collect();
    while !parts.is_empty() {
        let path = format!("{base}\\{}", parts.join("\\"));
        let Some(key) = open(&path, KEY_READ) else { return };
        let (mut subkeys, mut values) = (0u32, 0u32);
        let ok = unsafe {
            let r = RegQueryInfoKeyW(key, None, None, None, Some(&mut subkeys), None, None, Some(&mut values), None, None, None, None);
            let _ = RegCloseKey(key);
            r.is_ok()
        };
        if !ok || subkeys > 0 || values > 0 {
            return;
        }
        if unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(path)) }.is_err() {
            return;
        }
        parts.pop();
    }
}

/// Turns the banners of `aumids` off, keeping in `kept` what each had (an
/// app already in `kept` is already ours). True when something changed.
pub fn take<'a>(base: &str, aumids: impl IntoIterator<Item = &'a str>, kept: &mut BTreeMap<String, Kept>) -> bool {
    let mut changed = false;
    for aumid in aumids {
        if aumid.is_empty() || kept.contains_key(aumid) {
            continue;
        }
        let was = Kept { before: read(base, aumid), created: !exists(base, aumid) };
        match write(base, aumid, Some(0)) {
            Ok(()) => {
                kept.insert(aumid.to_string(), was);
                changed = true;
            }
            Err(err) => crate::store::log(&format!("banners: {err}")),
        }
    }
    changed
}

/// Puts back what the apps in `kept` had (only those `which` picks) and
/// forgets them. True when something changed.
pub fn give_back(base: &str, kept: &mut BTreeMap<String, Kept>, which: impl Fn(&str) -> bool) -> bool {
    let back: Vec<String> = kept.keys().filter(|a| which(a)).cloned().collect();
    for aumid in &back {
        let Some(was) = kept.remove(aumid) else { continue };
        if let Err(err) = write(base, aumid, was.before) {
            crate::store::log(&format!("banners: {err}"));
        }
        if was.created {
            remove_empty(base, aumid);
        }
    }
    !back.is_empty()
}

/// Calls `changed` (on a thread of its own) whenever an app's key comes or
/// goes under `base`: Windows makes one for a newly installed app.
pub fn watch(base: &'static str, changed: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("fubunotch-banners".into())
        .spawn(move || {
            let Some(key) = open(base, KEY_NOTIFY) else { return };
            loop {
                let r = unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_NAME, None, false) };
                if r.is_err() {
                    break;
                }
                // Several keys often come at once: one call for them.
                std::thread::sleep(Duration::from_millis(500));
                changed();
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch key of our own, never Windows' real settings.
    const TEST: &str = "Software\\FubuNotch v2 tests\\banners";

    #[test]
    fn take_then_give_back_restores_each_app_exactly() {
        write(TEST, "App.Had.Banners", Some(1)).unwrap();
        write(TEST, "App.Had.None", Some(1)).unwrap();
        write(TEST, "App.Had.None", None).unwrap();
        write(TEST, "App.Was.Off", Some(0)).unwrap();
        let mut kept = BTreeMap::new();
        assert!(take(TEST, ["App.Had.Banners", "App.Had.None", "App.Was.Off"], &mut kept));
        for a in ["App.Had.Banners", "App.Had.None", "App.Was.Off"] {
            assert_eq!(read(TEST, a), Some(0), "{a} off");
        }
        assert!(!take(TEST, ["App.Had.Banners"], &mut kept), "already ours: kept value stays the first one");
        assert!(give_back(TEST, &mut kept, |_| true));
        assert_eq!(read(TEST, "App.Had.Banners"), Some(1));
        assert_eq!(read(TEST, "App.Had.None"), None, "no value before: none after");
        assert!(exists(TEST, "App.Had.None"), "its key was there before: it stays");
        assert_eq!(read(TEST, "App.Was.Off"), Some(0));
        assert!(kept.is_empty());
    }

    #[test]
    fn a_key_made_for_an_app_goes_again_with_its_parents() {
        let mut kept = BTreeMap::new();
        let aumid = "{TEST-GUID}\\Nested\\app.exe";
        assert!(!exists(TEST, aumid));
        take(TEST, [aumid, "Never.Seen.App"], &mut kept);
        assert_eq!(read(TEST, aumid), Some(0));
        assert!(kept[aumid].created);
        give_back(TEST, &mut kept, |_| true);
        assert!(!exists(TEST, aumid));
        assert!(!exists(TEST, "{TEST-GUID}"), "the parents made with it go too");
        assert!(!exists(TEST, "Never.Seen.App"));
    }

    #[test]
    fn give_back_one_app_only() {
        let mut kept = BTreeMap::new();
        write(TEST, "One.A", Some(1)).unwrap();
        write(TEST, "One.B", Some(1)).unwrap();
        take(TEST, ["One.A", "One.B"], &mut kept);
        give_back(TEST, &mut kept, |a| a == "One.A");
        assert_eq!(read(TEST, "One.A"), Some(1));
        assert_eq!(read(TEST, "One.B"), Some(0));
        assert!(kept.contains_key("One.B"));
        give_back(TEST, &mut kept, |_| true);
    }

    #[test]
    fn the_start_menu_lists_apps() {
        let apps = read_start_apps();
        assert!(apps.len() > 5, "{apps:?}");
        assert!(apps.iter().all(|a| !a.contains("://")));
    }
}
