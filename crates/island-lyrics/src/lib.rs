//! Synced lyrics for the Music page, ported from FubuNotch v1 (our own code
//! there, not Coucou's): LRCLIB first, YouTube Music when allowed and LRCLIB
//! has none, then Japanese lines in romaji. Blocking (ureq): call it from a
//! thread of its own. Answers are kept in memory per song.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

pub mod romaji;
pub mod ytm;

const LRCLIB: &str = "https://lrclib.net";
/// An LRC file is a few KB; anything past this is not lyrics.
const ANSWER_LIMIT: u64 = 512 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const CACHE_SIZE: usize = 64;
/// A search hit may be this many seconds longer or shorter than the track.
const DURATION_SLACK: f64 = 3.0;

/// One line of the lyrics, from `time` (seconds into the track).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Line {
    pub time: f64,
    pub text: String,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Found,
    /// Nobody has synced lyrics for it.
    None,
    Instrumental,
    /// A service could not be reached; not remembered.
    Error,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub status: Status,
    pub lines: Vec<Line>,
    /// The length of the recording the lines were timed for (seconds; 0 when
    /// unknown). A video longer than it usually has an intro: the app shifts
    /// the lines by the difference.
    pub duration: f64,
}

impl Lyrics {
    pub fn of(status: Status) -> Self {
        Lyrics { status, lines: Vec::new(), duration: 0.0 }
    }
}

/// What is asked about.
#[derive(Clone, Debug)]
pub struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Seconds; 0 when the player doesn't say.
    pub duration: f64,
}

impl Query {
    pub fn key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.title.trim().to_lowercase(),
            self.artist.trim().to_lowercase(),
            self.album.trim().to_lowercase(),
            self.duration.round() as i64
        )
    }
}

/// Which sources may be asked.
#[derive(Clone, Copy, Debug)]
pub struct Sources {
    pub lrclib: bool,
    pub youtube: bool,
    pub romaji: bool,
}

// ── Log ───────────────────────────────────────────────────────────────────────

static LOGGER: OnceLock<Box<dyn Fn(&str) + Send + Sync>> = OnceLock::new();

/// Where "why no lyrics" lines go (the app's log file).
pub fn set_logger(f: impl Fn(&str) + Send + Sync + 'static) {
    let _ = LOGGER.set(Box::new(f));
}

pub(crate) fn log(line: String) {
    match LOGGER.get() {
        Some(f) => f(&line),
        None => eprintln!("{line}"),
    }
}

// ── LRC ───────────────────────────────────────────────────────────────────────

/// "mm:ss", "mm:ss.xx", "mm:ss.xxx" or "mm:ss:xx" → seconds.
fn timestamp(tag: &str) -> Option<f64> {
    let (minutes, rest) = tag.split_once(':')?;
    let minutes: u32 = minutes.trim().parse().ok()?;
    let rest = rest.replacen(':', ".", 1);
    let seconds: f64 = rest.trim().parse().ok()?;
    (seconds.is_finite() && (0.0..60.0).contains(&seconds)).then(|| minutes as f64 * 60.0 + seconds)
}

/// LRC text → its lines in time order. A line may carry several timestamps;
/// `[offset:+250]` moves every line 250 ms earlier; other tags and untimed
/// lines are dropped. An empty line is kept: it is a pause.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut offset_ms = 0.0;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some(end) = after.find(']') else { break };
            let tag = &after[..end];
            if let Some(time) = timestamp(tag) {
                times.push(time);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                if let Ok(ms) = value.trim().parse::<f64>() {
                    offset_ms = ms;
                }
            }
            rest = after[end + 1..].trim_start();
        }
        for time in times {
            lines.push(Line { time, text: rest.trim().to_string() });
        }
    }
    let shift = offset_ms / 1000.0;
    for line in &mut lines {
        line.time = (line.time - shift).max(0.0);
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

/// How much later than its lyrics a recording runs: a video up to 8 s longer
/// than the recording the lyrics were timed for is taken to have an intro of
/// that length (MVs often do). 0 when either length is unknown or the video
/// is shorter (no way to tell where the cut is).
pub fn intro_shift(playing: f64, timed_for: f64) -> f64 {
    let d = playing - timed_for;
    if playing > 0.0 && timed_for > 0.0 && d > 0.0 && d <= 8.0 { d } else { 0.0 }
}

/// The line sung at `position` seconds (None before the first).
pub fn line_at(lines: &[Line], position: f64) -> Option<usize> {
    lines.iter().rposition(|l| l.time <= position)
}

// ── LRCLIB ────────────────────────────────────────────────────────────────────

fn from_record(record: &Value) -> Option<Lyrics> {
    if record.get("instrumental").and_then(Value::as_bool) == Some(true) {
        return Some(Lyrics::of(Status::Instrumental));
    }
    let synced = record.get("syncedLyrics").and_then(Value::as_str)?;
    let lines = parse_lrc(synced);
    if lines.iter().all(|l| l.text.is_empty()) {
        return None;
    }
    let duration = record.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    Some(Lyrics { status: Status::Found, lines, duration })
}

/// The best search hit: synced lyrics of about the track's length, closest first.
pub fn pick_search(hits: &[Value], duration: f64) -> Option<Lyrics> {
    let length = |hit: &Value| hit.get("duration").and_then(Value::as_f64);
    let near = |hit: &Value| duration <= 0.0 || length(hit).is_none_or(|d| (d - duration).abs() <= DURATION_SLACK);
    let mut candidates: Vec<&Value> = hits.iter().filter(|h| near(h)).collect();
    candidates.sort_by(|a, b| {
        let gap = |h: &Value| length(h).map_or(f64::MAX, |d| (d - duration).abs());
        gap(a).total_cmp(&gap(b))
    });
    let mut instrumental = None;
    for hit in candidates {
        match from_record(hit) {
            Some(l) if l.status == Status::Found => return Some(l),
            Some(l) => instrumental = instrumental.or(Some(l)),
            None => {}
        }
    }
    instrumental
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("FubuNotch-v2/", env!("CARGO_PKG_VERSION"), " (personal)"))
        .http_status_as_error(false)
        .build()
        .into()
}

/// GET → Ok(None) for a 404, Err when it could not be asked or read.
fn get_json(agent: &ureq::Agent, url: &str, query: &[(&str, &str)]) -> Result<Option<Value>, ()> {
    let mut req = agent.get(url);
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut response = req.call().map_err(|err| log(format!("lyrics error: LRCLIB {url}: {err}")))?;
    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        log(format!("lyrics error: LRCLIB {url}: HTTP {status}"));
        return Err(());
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(ANSWER_LIMIT)
        .read_to_string()
        .map_err(|err| log(format!("lyrics error: LRCLIB {url}: {err}")))?;
    serde_json::from_str(&body).map(Some).map_err(|err| log(format!("lyrics error: LRCLIB {url}: not JSON ({err})")))
}

/// LRCLIB: the exact track first, then a search.
pub fn lrclib(query: &Query) -> Lyrics {
    let agent = agent();
    let (title, artist, album) = (query.title.trim(), query.artist.trim(), query.album.trim());
    if title.is_empty() {
        return Lyrics::of(Status::None);
    }
    if !album.is_empty() && !artist.is_empty() && query.duration > 0.0 {
        let duration = (query.duration.round() as i64).to_string();
        let params = [("track_name", title), ("artist_name", artist), ("album_name", album), ("duration", duration.as_str())];
        match get_json(&agent, &format!("{LRCLIB}/api/get"), &params) {
            Ok(Some(record)) => {
                if let Some(lyrics) = from_record(&record) {
                    return lyrics;
                }
            }
            Ok(None) => {}
            Err(()) => return Lyrics::of(Status::Error),
        }
    }
    let mut params = vec![("track_name", title)];
    if !artist.is_empty() {
        params.push(("artist_name", artist));
    }
    match get_json(&agent, &format!("{LRCLIB}/api/search"), &params) {
        Ok(Some(Value::Array(hits))) => pick_search(&hits, query.duration).unwrap_or(Lyrics::of(Status::None)),
        Ok(_) => Lyrics::of(Status::None),
        Err(()) => Lyrics::of(Status::Error),
    }
}

// ── The chain ─────────────────────────────────────────────────────────────────

static CACHE: Mutex<VecDeque<(String, Lyrics)>> = Mutex::new(VecDeque::new());

/// LRCLIB, then YouTube Music when allowed (also when LRCLIB is down), then
/// romaji for Japanese lines. Blocking.
pub fn resolve(query: &Query, sources: Sources) -> Lyrics {
    if !sources.lrclib && !sources.youtube {
        return Lyrics::of(Status::None);
    }
    let key = format!("{}\u{1f}{}{}", query.key(), sources.youtube as u8, sources.romaji as u8);
    if let Some((_, hit)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == key) {
        return hit.clone();
    }
    let mut lyrics = if sources.lrclib { lrclib(query) } else { Lyrics::of(Status::None) };
    let mut trail = format!("lyrics: \"{}\" ({:.0} s): LRCLIB {:?}", query.title, query.duration, lyrics.status);
    if sources.youtube && matches!(lyrics.status, Status::None | Status::Error) {
        let fallback = ytm::fetch(&agent(), query);
        trail.push_str(&format!(", YouTube Music {:?}", fallback.status));
        if fallback.status == Status::Found {
            lyrics = fallback;
        }
    }
    log(format!("{trail}, {} lines", lyrics.lines.len()));
    if sources.romaji && lyrics.lines.iter().any(|l| romaji::is_japanese(&l.text)) {
        for line in &mut lyrics.lines {
            line.text = romaji::line_to_romaji(&line.text);
        }
    }
    if lyrics.status != Status::Error {
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|(k, _)| *k != key);
        cache.push_back((key, lyrics.clone()));
        while cache.len() > CACHE_SIZE {
            cache.pop_front();
        }
    }
    lyrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(time: f64, text: &str) -> Line {
        Line { time, text: text.into() }
    }

    #[test]
    fn lrc_lines_are_timed_sorted_and_offset() {
        let lines = parse_lrc("[ar:x]\n[00:02.00]b\n[00:01.50][00:03.00]a\nuntimed\n[offset:+500]");
        assert_eq!(lines, vec![line(1.0, "a"), line(1.5, "b"), line(2.5, "a")]);
    }

    #[test]
    fn an_mv_longer_than_the_song_has_an_intro() {
        assert_eq!(intro_shift(168.0, 166.0), 2.0, "Deep Dive: MV 2:48, song 2:46");
        assert_eq!(intro_shift(166.0, 168.0), 0.0, "shorter: no guess");
        assert_eq!(intro_shift(200.0, 166.0), 0.0, "far off: not an intro");
        assert_eq!(intro_shift(168.0, 0.0), 0.0, "unknown length");
    }

    #[test]
    fn the_line_at_a_position() {
        let lines = vec![line(1.0, "a"), line(2.0, "b"), line(4.0, "c")];
        assert_eq!(line_at(&lines, 0.5), None);
        assert_eq!(line_at(&lines, 2.0), Some(1));
        assert_eq!(line_at(&lines, 99.0), Some(2));
    }

    #[test]
    fn a_search_picks_synced_lyrics_of_about_the_length() {
        let hits = vec![
            json!({ "duration": 250.0, "syncedLyrics": "[00:01.00]Wrong version" }),
            json!({ "duration": 213.0, "syncedLyrics": null, "plainLyrics": "plain" }),
            json!({ "duration": 212.5, "syncedLyrics": "[00:01.00]Closest" }),
        ];
        assert_eq!(pick_search(&hits, 212.0).unwrap().lines[0].text, "Closest");
        assert_eq!(pick_search(&hits, 100.0), None);
    }

    #[test]
    fn no_source_allowed_asks_nothing() {
        let q = Query { title: "x".into(), artist: String::new(), album: String::new(), duration: 0.0 };
        assert_eq!(resolve(&q, Sources { lrclib: false, youtube: false, romaji: true }).status, Status::None);
    }
}
