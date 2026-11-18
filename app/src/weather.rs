// Weather for the Dashboard (as Poni's): now and today's high / low at the
// Prayer module's place, from Open-Meteo (free, no account, no key). Off
// until the user turns it on (the "weather" module): network only for what
// the user switched on. Read on a thread, at most every REFRESH_MS.

use serde_json::Value;

/// How old a reading may get before the Dashboard asks again.
pub const REFRESH_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    /// °C now, today's high and low.
    pub now: f32,
    pub high: f32,
    pub low: f32,
    /// WMO weather code.
    pub code: u32,
    pub day: bool,
}

/// Asks Open-Meteo for `lat`, `lon` (blocking: call it off the UI thread).
pub fn fetch(lat: f64, lon: f64) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.3}&longitude={lon:.3}&current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    );
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(15))).user_agent("kanmuri").build().into();
    let mut r = agent.get(&url).call().map_err(|e| e.to_string())?;
    let body = r.body_mut().with_config().limit(64 * 1024).read_to_string().map_err(|e| e.to_string())?;
    parse(&body).ok_or_else(|| "unexpected answer".into())
}

pub fn parse(body: &str) -> Option<Weather> {
    let v: Value = serde_json::from_str(body).ok()?;
    let cur = v.get("current")?;
    let daily = v.get("daily")?;
    let first = |k: &str| daily.get(k)?.as_array()?.first()?.as_f64();
    Some(Weather {
        now: cur.get("temperature_2m")?.as_f64()? as f32,
        code: cur.get("weather_code")?.as_u64()? as u32,
        day: cur.get("is_day").and_then(Value::as_u64).unwrap_or(1) == 1,
        high: first("temperature_2m_max")? as f32,
        low: first("temperature_2m_min")? as f32,
    })
}

/// The WMO code in a few words.
pub fn describe(code: u32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51 | 53 | 55 | 56 | 57 => "Drizzle",
        61 | 63 | 66 => "Rain",
        65 | 67 => "Heavy rain",
        71 | 73 | 75 | 77 => "Snow",
        80 | 81 => "Showers",
        82 => "Heavy showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, hail",
        _ => "—",
    }
}

/// A Segoe Fluent glyph for the code: the sun (clear by day), the moon (clear
/// by night), else a cloud. Only glyphs the font is known to have (a missing
/// one draws nothing; the words say the rest).
pub fn glyph(code: u32, day: bool) -> &'static str {
    match (code, day) {
        (0 | 1, true) => "\u{E706}",
        (0 | 1, false) => "\u{E708}",
        _ => "\u{E753}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_meteos_answer() {
        let body = r#"{"current":{"temperature_2m":29.4,"weather_code":2,"is_day":1},"daily":{"temperature_2m_max":[32.1],"temperature_2m_min":[24.8]}}"#;
        let w = parse(body).unwrap();
        assert_eq!((w.now, w.high, w.low, w.code, w.day), (29.4, 32.1, 24.8, 2, true));
        assert_eq!(describe(2), "Partly cloudy");
        assert!(parse("{}").is_none());
    }

    /// Asks Open-Meteo for real (network): cargo test -p kanmuri weather_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn weather_live() {
        let w = fetch(-6.2, 106.8).unwrap();
        println!("{w:?} {}", describe(w.code));
        assert!(w.low <= w.high);
    }
}
