// "Notifications only on the island": Windows' own pop-up banners turned off
// app by app, the switch Windows Settings → Notifications → <app> → "Show
// notification banners" writes (ShowBanner = 0 under the app's key). The
// notifications still land in Windows' notification centre, where FubuNotch
// reads them. What each app had before is kept, to put back exactly.
// Per user (HKCU), no admin rights.

use std::collections::BTreeMap;

use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    RRF_RT_REG_DWORD,
};

/// Where Windows keeps each app's notification settings.
pub const SETTINGS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings";
const VALUE: &str = "ShowBanner";

/// The apps Windows has notification settings for (their AUMIDs).
pub fn apps(base: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(base), None, KEY_READ, &mut key).is_err() {
            return out;
        }
        let mut name = [0u16; 512];
        for i in 0.. {
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// The app's ShowBanner, or None when it has none (Windows then shows banners).
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

/// Sets the app's ShowBanner, or removes it (None).
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

/// Turns the banners of `aumids` off, keeping in `kept` what each had (an
/// app already in `kept` is already ours). True when something changed.
pub fn take<'a>(base: &str, aumids: impl IntoIterator<Item = &'a str>, kept: &mut BTreeMap<String, Option<u32>>) -> bool {
    let mut changed = false;
    for aumid in aumids {
        if aumid.is_empty() || kept.contains_key(aumid) {
            continue;
        }
        let before = read(base, aumid);
        match write(base, aumid, Some(0)) {
            Ok(()) => {
                kept.insert(aumid.to_string(), before);
                changed = true;
            }
            Err(err) => crate::store::log(&format!("banners: {err}")),
        }
    }
    changed
}

/// Puts back what the apps in `kept` had (only those `which` picks) and
/// forgets them. True when something changed.
pub fn give_back(base: &str, kept: &mut BTreeMap<String, Option<u32>>, which: impl Fn(&str) -> bool) -> bool {
    let back: Vec<String> = kept.keys().filter(|a| which(a)).cloned().collect();
    for aumid in &back {
        let before = kept.remove(aumid).flatten();
        if let Err(err) = write(base, aumid, before) {
            crate::store::log(&format!("banners: {err}"));
        }
    }
    !back.is_empty()
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
        assert_eq!(read(TEST, "App.Was.Off"), Some(0));
        assert!(kept.is_empty());
        assert!(apps(TEST).iter().any(|a| a == "App.Had.Banners"));
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
}
