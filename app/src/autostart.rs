// "Start with Windows": a value under HKCU\...\Run pointing at this exe.
// Per user, no admin rights.

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("FubuNotch v2");

pub fn set(on: bool) -> Result<(), String> {
    unsafe {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            // Quoted: the path may hold spaces ("Development Folder").
            let command = HSTRING::from(format!("\"{}\"", exe.display()));
            let bytes = std::slice::from_raw_parts(command.as_ptr() as *const u8, (command.len() + 1) * 2);
            RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, REG_SZ.0, Some(bytes.as_ptr() as *const _), bytes.len() as u32)
                .ok()
                .map_err(|e| e.to_string())
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE);
            // Already gone is fine.
            if r.is_ok() || r.0 == 2 {
                Ok(())
            } else {
                Err(format!("{r:?}"))
            }
        }
    }
}
