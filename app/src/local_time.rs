// The local time from Windows (no time-zone crate needed).

use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

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

/// Milliseconds until the next second starts (a countdown's tick).
pub fn ms_to_next_second() -> u64 {
    let t = unsafe { GetLocalTime() };
    1000 - t.wMilliseconds as u64 % 1000
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

/// Today's date and the seconds since midnight, now: (year, month, day, seconds).
pub fn now_parts() -> (i32, u32, u32, u32) {
    let t = unsafe { GetLocalTime() };
    (t.wYear as i32, t.wMonth as u32, t.wDay as u32, t.wHour as u32 * 3600 + t.wMinute as u32 * 60 + t.wSecond as u32)
}

/// Days since 1970-01-01 for a date (Howard Hinnant's days_from_civil).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `n` days after a date.
pub fn add_days(y: i32, m: u32, d: u32, n: i64) -> (i32, u32, u32) {
    let z = days_from_civil(y, m, d) + n + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32, month, day)
}

/// Windows' offset from UTC now, in hours (7 for WIB): local time minus
/// UTC, to the quarter hour.
pub fn utc_offset_hours() -> f64 {
    let (l, u) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let minutes = |t: &windows::Win32::Foundation::SYSTEMTIME| {
        days_from_civil(t.wYear as i32, t.wMonth as u32, t.wDay as u32) * 1440 + t.wHour as i64 * 60 + t.wMinute as i64
    };
    let diff = (minutes(&l) - minutes(&u)) as f64;
    (diff / 15.0).round() * 15.0 / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_add_up() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(add_days(2026, 11, 18, 1), (2026, 11, 19));
        assert_eq!(add_days(2026, 12, 31, 1), (2027, 1, 1));
        assert_eq!(add_days(2028, 2, 28, 1), (2028, 2, 29));
        assert_eq!(add_days(2027, 2, 28, 1), (2027, 3, 1));
        assert!((-12.0..=14.0).contains(&utc_offset_hours()));
    }
}
