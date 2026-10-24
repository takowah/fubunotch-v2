// The local time from Windows (no time-zone crate needed).

use windows::Win32::System::SystemInformation::GetLocalTime;

/// "14:05".
pub fn clock_text() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// Milliseconds until the next minute starts (the clock's one wake a minute).
pub fn ms_to_next_minute() -> u64 {
    let t = unsafe { GetLocalTime() };
    (60 - t.wSecond as u64) * 1000 - t.wMilliseconds as u64
}

/// "3:07" for a number of seconds.
pub fn mm_ss(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// "2026-10-23 22:41:07" for the log.
pub fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// "2026-10-23", today.
pub fn today() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// Minutes since midnight, now.
pub fn minute_of_day() -> u32 {
    let t = unsafe { GetLocalTime() };
    t.wHour as u32 * 60 + t.wMinute as u32
}

/// "14:05" for minutes since midnight (wraps past midnight).
pub fn hh_mm(minute: u32) -> String {
    let m = minute % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// "Fri 23 Oct", today.
pub fn date_text() -> String {
    let t = unsafe { GetLocalTime() };
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {} {}", DAYS[t.wDayOfWeek as usize % 7], t.wDay, MONTHS[(t.wMonth as usize).saturating_sub(1) % 12])
}
