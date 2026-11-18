// System stats for the closed island's sides (as Poni's): CPU load, memory in
// use, the system disk's fill. Read on demand, only while a side shows one
// and the island is up (main.rs `stats_timer`); nothing runs while hidden.

use std::sync::Mutex;

/// The last (idle, kernel, user) times, to turn the next reading into a load.
static LAST_CPU: Mutex<Option<(u64, u64, u64)>> = Mutex::new(None);

fn ft(t: windows::Win32::Foundation::FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// CPU load in % since the last call (None on the first one).
pub fn cpu_percent() -> Option<u32> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetSystemTimes;
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };
    let now = (ft(idle), ft(kernel), ft(user));
    let mut last = LAST_CPU.lock().unwrap_or_else(|e| e.into_inner());
    let before = last.replace(now)?;
    Some(load(before, now))
}

/// Kernel time includes idle time: busy = (kernel + user) - idle.
fn load(a: (u64, u64, u64), b: (u64, u64, u64)) -> u32 {
    let idle = b.0.saturating_sub(a.0);
    let total = b.1.saturating_sub(a.1) + b.2.saturating_sub(a.2);
    if total == 0 {
        return 0;
    }
    ((total.saturating_sub(idle)) * 100 / total).min(100) as u32
}

/// Memory in use, %.
pub fn memory_percent() -> Option<u32> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut m).ok()? };
    Some(m.dwMemoryLoad)
}

/// How full the system drive is, %.
pub fn disk_percent() -> Option<u32> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\";
    let (mut free, mut total) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(drive), None, Some(&mut total), Some(&mut free)).ok()? };
    (total > 0).then(|| ((total - free) * 100 / total) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_is_busy_time_over_all_time() {
        // Kernel 700 (of which 250 idle) + user 300 = 1000 ticks, 750 busy: 75 %.
        assert_eq!(load((0, 0, 0), (250, 700, 300)), 75);
        assert_eq!(load((5, 5, 5), (5, 5, 5)), 0, "no time passed");
    }

    #[test]
    fn readings_are_percentages() {
        assert!(memory_percent().is_some_and(|p| p <= 100));
        assert!(disk_percent().is_some_and(|p| p <= 100));
        let _ = cpu_percent();
        assert!(cpu_percent().is_some_and(|p| p <= 100));
    }
}
