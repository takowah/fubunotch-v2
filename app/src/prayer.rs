// The Prayer module's arithmetic: the day's prayer times and the qibla for a
// place, worked out here (no network). The sun's position is the usual
// low-precision formula (praytimes.org); the method is Kemenag's: Subuh at the
// sun 20° below the horizon, Isya at 18°, Ashar at one shadow length (Shafi'i),
// then Kemenag's ihtiyat and rounding up to the minute. The ihtiyat were fitted
// on Kemenag's own schedule for Kota Jakarta (June and November 2026, from
// api.myquran.com, 18 Nov 2026): 322 of 360 times exact, the rest one minute
// off; Imsak is always Subuh − 10.

/// Which prayer (or mark) a time is, in the day's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Imsak,
    Subuh,
    Terbit,
    Dzuhur,
    Ashar,
    Maghrib,
    Isya,
}

impl Which {
    pub fn name(self) -> &'static str {
        match self {
            Which::Imsak => "Imsak",
            Which::Subuh => "Subuh",
            Which::Terbit => "Terbit",
            Which::Dzuhur => "Dzuhur",
            Which::Ashar => "Ashar",
            Which::Maghrib => "Maghrib",
            Which::Isya => "Isya",
        }
    }
    /// The five prayers (Imsak and Terbit are marks, not prayers).
    pub fn is_prayer(self) -> bool {
        !matches!(self, Which::Imsak | Which::Terbit)
    }
}

/// A day's times, minutes after local midnight.
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub times: Vec<(Which, u32)>,
}

impl Day {
    pub fn get(&self, w: Which) -> u32 {
        self.times.iter().find(|(x, _)| *x == w).map(|(_, m)| *m).unwrap_or(0)
    }
    /// The five prayers only.
    pub fn prayers(&self) -> Vec<(Which, u32)> {
        self.times.iter().copied().filter(|(w, _)| w.is_prayer()).collect()
    }
}

/// "04:04" for minutes after midnight.
pub fn hh_mm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60 % 24, m % 60)
}

fn sin(d: f64) -> f64 {
    d.to_radians().sin()
}
fn cos(d: f64) -> f64 {
    d.to_radians().cos()
}
fn tan(d: f64) -> f64 {
    d.to_radians().tan()
}

fn julian(mut y: i32, mut m: u32, d: u32) -> f64 {
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor() + (30.6001 * (m as f64 + 1.0)).floor() + d as f64 + b - 1524.5
}

/// The sun's declination and the equation of time (hours) at a Julian day.
fn sun(jd: f64) -> (f64, f64) {
    let d = jd - 2451545.0;
    let g = (357.529 + 0.98560028 * d).rem_euclid(360.0);
    let q = (280.459 + 0.98564736 * d).rem_euclid(360.0);
    let l = (q + 1.915 * sin(g) + 0.020 * sin(2.0 * g)).rem_euclid(360.0);
    let e = 23.439 - 0.00000036 * d;
    let ra = (cos(e) * sin(l)).atan2(cos(l)).to_degrees().rem_euclid(360.0) / 15.0;
    let decl = (sin(e) * sin(l)).asin().to_degrees();
    let eqt = (q / 15.0 - ra + 12.0).rem_euclid(24.0) - 12.0;
    (decl, eqt)
}

/// Kemenag's times for a date at a place; `tz_hours` is the local offset (7 for WIB).
pub fn day(year: i32, month: u32, date: u32, lat: f64, lon: f64, tz_hours: f64) -> Day {
    let jd = julian(year, month, date) - lon / (15.0 * 24.0);
    // Hours from noon until the sun is `angle` below the horizon, at about `t` (day hours).
    let span = |angle: f64, t: f64| {
        let (decl, _) = sun(jd + t / 24.0);
        let c = (-sin(angle) - sin(decl) * sin(lat)) / (cos(decl) * cos(lat));
        c.clamp(-1.0, 1.0).acos().to_degrees() / 15.0
    };
    let asr = |t: f64| {
        let (decl, _) = sun(jd + t / 24.0);
        let angle = -(1.0 / (1.0 + tan((lat - decl).abs()))).atan().to_degrees();
        let c = (-sin(angle) - sin(decl) * sin(lat)) / (cos(decl) * cos(lat));
        c.clamp(-1.0, 1.0).acos().to_degrees() / 15.0
    };
    // Two passes: each time is worked out again with the sun at that time.
    let mut t = [5.0, 6.0, 12.0, 13.0, 18.0, 18.0];
    for _ in 0..2 {
        let (_, eqt) = sun(jd + t[2] / 24.0);
        let noon = 12.0 - eqt;
        t = [noon - span(20.0, t[0]), noon - span(0.833, t[1]), noon, noon + asr(t[3]), noon + span(0.833, t[4]), noon + span(18.0, t[5])];
    }
    // Kemenag's ihtiyat (minutes), then up to the whole minute.
    const IHTIYAT: [f64; 6] = [2.0, -4.0, 3.0, 2.0, 3.0, 2.0];
    let m: Vec<u32> = t.iter().zip(IHTIYAT).map(|(h, a)| ((h + tz_hours - lon / 15.0) * 60.0 + a).ceil().max(0.0) as u32).collect();
    Day {
        times: vec![
            (Which::Imsak, m[0].saturating_sub(10)),
            (Which::Subuh, m[0]),
            (Which::Terbit, m[1]),
            (Which::Dzuhur, m[2]),
            (Which::Ashar, m[3]),
            (Which::Maghrib, m[4]),
            (Which::Isya, m[5]),
        ],
    }
}

/// The qibla's bearing from north (degrees, clockwise) and its compass point ("WNW").
pub fn qibla(lat: f64, lon: f64) -> (u32, &'static str) {
    const KAABA: (f64, f64) = (21.4225, 39.8262);
    let b = sin(KAABA.1 - lon).atan2(cos(lat) * tan(KAABA.0) - sin(lat) * cos(KAABA.1 - lon)).to_degrees().rem_euclid(360.0);
    const POINTS: [&str; 16] = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    (b.round() as u32 % 360, POINTS[((b + 11.25) / 22.5) as usize % 16])
}

/// Places to pick from (Settings → Prayer): Kemenag's regencies and cities,
/// by the coordinates of their seat.
pub const PLACES: &[(&str, f64, f64)] = &[
    ("Kab. Bekasi", -6.2615, 107.1527),
    ("Kota Bekasi", -6.2383, 106.9756),
    ("Kota Jakarta", -6.1754, 106.8272),
    ("Kota Depok", -6.4025, 106.7942),
    ("Kota Bogor", -6.5971, 106.8060),
    ("Kota Tangerang", -6.1783, 106.6319),
    ("Kota Bandung", -6.9175, 107.6191),
    ("Kota Surabaya", -7.2575, 112.7521),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: &Day) -> Vec<String> {
        d.times.iter().map(|(_, m)| hh_mm(*m)).collect()
    }

    fn within_a_minute(got: &Day, want: [&str; 7]) {
        for ((w, g), s) in got.times.iter().zip(want) {
            let m = s[..2].parse::<i64>().unwrap() * 60 + s[3..].parse::<i64>().unwrap();
            assert!((*g as i64 - m).abs() <= 1, "{}: got {} want {s} ({:?})", w.name(), hh_mm(*g), at(got));
        }
    }

    #[test]
    fn kemenag_jakarta() {
        // Kemenag (api.myquran.com, Kota Jakarta, id 1301).
        let d = day(2026, 11, 1, -6.17, 106.85, 7.0);
        within_a_minute(&d, ["03:59", "04:09", "05:23", "11:40", "14:55", "17:49", "19:00"]);
        assert_eq!(d.get(Which::Subuh) - d.get(Which::Imsak), 10);
    }

    #[test]
    fn bekasi_as_poni_showed_it() {
        // Kab. Bekasi, 18 Nov 2026, as another island app showed it.
        let d = day(2026, 11, 18, -6.2615, 107.1527, 7.0);
        within_a_minute(&d, ["03:54", "04:04", "05:21", "11:40", "15:01", "17:52", "19:05"]);
        assert_eq!(qibla(-6.2615, 107.1527), (295, "WNW"));
    }

    #[test]
    fn five_prayers() {
        let d = day(2026, 6, 21, -6.17, 106.85, 7.0);
        let names: Vec<&str> = d.prayers().iter().map(|(w, _)| w.name()).collect();
        assert_eq!(names, ["Subuh", "Dzuhur", "Ashar", "Maghrib", "Isya"]);
        assert!(d.times.windows(2).all(|w| w[0].1 < w[1].1));
    }
}
