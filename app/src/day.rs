// The Day Progress module's arithmetic: how much of the waking day, the
// week (Monday 00:00 to the next Monday), the month and the year has gone.
// Pure: the caller passes the date and the time.

use crate::local_time::days_from_civil;

const DAY_S: f64 = 86_400.0;

/// How far `now` (seconds after midnight) is between wake and bedtime
/// (minutes after midnight), 0..1: 0 before waking, 1 after bedtime.
pub fn day_fraction(now_s: u32, wake_min: u32, bed_min: u32) -> f32 {
    let (a, b) = (wake_min as f64 * 60.0, bed_min as f64 * 60.0);
    if b <= a {
        return 0.0;
    }
    ((now_s as f64 - a) / (b - a)).clamp(0.0, 1.0) as f32
}

/// Whole minutes until bedtime (0 once it has passed).
pub fn left_until(now_s: u32, at_min: u32) -> u32 {
    (at_min * 60).saturating_sub(now_s) / 60
}

/// Monday 0 .. Sunday 6 (1970-01-01 was a Thursday).
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) + 3).rem_euclid(7) as u32
}

/// The week from Monday 00:00 to the next Monday.
pub fn week_fraction(y: i32, m: u32, d: u32, now_s: u32) -> f32 {
    ((weekday(y, m, d) as f64 * DAY_S + now_s as f64) / (7.0 * DAY_S)).clamp(0.0, 1.0) as f32
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// The calendar month, from the 1st at 00:00 to the next 1st.
pub fn month_fraction(y: i32, m: u32, d: u32, now_s: u32) -> f32 {
    (((d - 1) as f64 * DAY_S + now_s as f64) / (days_in_month(y, m) as f64 * DAY_S)).clamp(0.0, 1.0) as f32
}

/// The calendar year, from 1 January 00:00 to the next.
pub fn year_fraction(y: i32, m: u32, d: u32, now_s: u32) -> f32 {
    let start = days_from_civil(y, 1, 1);
    let len = (days_from_civil(y + 1, 1, 1) - start) as f64;
    (((days_from_civil(y, m, d) - start) as f64 * DAY_S + now_s as f64) / (len * DAY_S)).clamp(0.0, 1.0) as f32
}

/// A fraction as a whole percent, rounded down (100 only when it is all gone).
pub fn percent(f: f32) -> u32 {
    ((f as f64 * 100.0 + 1e-6).floor() as u32).min(100)
}

/// "11h 11m", "45m", "3h 0m".
pub fn h_m(minutes: u32) -> String {
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h {}m", minutes / 60, minutes % 60)
    }
}

/// The ticks under the day's bar: every 3 hours from the first whole hour at
/// or after waking, before bedtime; (minute, where on the bar 0..1).
pub fn ticks(wake_min: u32, bed_min: u32) -> Vec<(u32, f32)> {
    let mut out = Vec::new();
    if bed_min <= wake_min {
        return out;
    }
    let mut t = wake_min.div_ceil(60) * 60;
    while t < bed_min {
        out.push((t, (t - wake_min) as f32 / (bed_min - wake_min) as f32));
        t += 180;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_from_wake_to_bedtime() {
        // 07:00–23:00 at 11:48, as Poni showed it: 30 %, 11h 12m left (11:48:00).
        let now = 11 * 3600 + 48 * 60;
        assert_eq!(percent(day_fraction(now, 420, 1380)), 30);
        assert_eq!(h_m(left_until(now, 1380)), "11h 12m");
        // Before waking: nothing gone; after bedtime: all of it, nothing left.
        assert_eq!(day_fraction(5 * 3600, 420, 1380), 0.0);
        assert_eq!(day_fraction(7 * 3600, 420, 1380), 0.0);
        assert_eq!(day_fraction(23 * 3600 + 30 * 60, 420, 1380), 1.0);
        assert_eq!(percent(day_fraction(22 * 3600 + 59 * 60, 420, 1380)), 99, "100 only at bedtime");
        assert_eq!(left_until(23 * 3600 + 30 * 60, 1380), 0);
        assert_eq!(h_m(45), "45m");
        assert_eq!(h_m(180), "3h 0m");
    }

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(weekday(2026, 11, 16), 0, "Monday");
        assert_eq!(weekday(2026, 11, 22), 6, "Sunday");
        assert_eq!(week_fraction(2026, 11, 16, 0), 0.0, "Monday 00:00");
        assert_eq!(percent(week_fraction(2026, 11, 18, 11 * 3600 + 48 * 60)), 35, "Wednesday noonish");
        assert_eq!(percent(week_fraction(2026, 11, 22, 86_399)), 99, "Sunday 23:59:59");
        assert_eq!(week_fraction(2026, 11, 19, 12 * 3600), 0.5, "Thursday noon is half the week");
    }

    #[test]
    fn months_and_years_by_the_calendar() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2028, 2), 29);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(month_fraction(2026, 11, 1, 0), 0.0);
        assert_eq!(month_fraction(2026, 11, 16, 0), 0.5, "30 days: the 16th at 00:00 is half");
        assert_eq!(percent(month_fraction(2026, 12, 31, 86_399)), 99);
        assert_eq!(year_fraction(2026, 1, 1, 0), 0.0);
        assert_eq!(percent(year_fraction(2026, 11, 18, 11 * 3600 + 48 * 60)), 88, "as Poni showed it");
        assert_eq!(percent(year_fraction(2026, 12, 31, 86_399)), 99);
        assert_eq!(year_fraction(2028, 7, 2, 0), 183.0 / 366.0, "a leap year has 366 days");
    }

    #[test]
    fn ticks_every_three_hours() {
        let t: Vec<u32> = ticks(420, 1380).iter().map(|x| x.0).collect();
        assert_eq!(t, [420, 600, 780, 960, 1140, 1320], "07:00 10:00 13:00 16:00 19:00 22:00");
        assert_eq!(ticks(420, 1380)[0].1, 0.0);
        assert_eq!(ticks(390, 1380)[0].0, 420, "06:30: the first whole hour");
        assert!(ticks(600, 600).is_empty());
    }
}
