// Synced lyrics from YouTube Music, when LRCLIB has none (lyrics.rs). Opt-in
// on its own switch (Settings → Integrations → Music), off by default: it asks
// music.youtube.com, through the same unofficial endpoints its apps use. The
// song is searched by title and artist, a hit of about the track's length is
// picked (not an instrumental unless the track is one), and its timed lyrics
// come from the Android app's lyrics page — the web page only has plain text.
//
// Unofficial means it may stop working whenever YouTube changes something;
// then the card simply has no lyrics, as before. Personal use only: this is
// not something to ship to the team or the public.

use serde_json::{json, Value};

use crate::{Line, Lyrics, Query, Status};

const BASE: &str = "https://music.youtube.com/youtubei/v1";
/// YouTube Music's "Songs" search filter.
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAoQCRADEAQQBQ%3D%3D";
/// A hit may be this many seconds longer or shorter than the track.
const DURATION_SLACK: f64 = 3.0;
/// Hits looked at, at most.
const HITS: usize = 5;
const ANSWER_LIMIT: u64 = 4 * 1024 * 1024;

fn web_client() -> Value {
    json!({ "clientName": "WEB_REMIX", "clientVersion": "1.20250101.01.00", "hl": "en" })
}

fn android_client() -> Value {
    json!({ "clientName": "ANDROID_MUSIC", "clientVersion": "7.27.52", "androidSdkVersion": 34, "hl": "en" })
}

fn call(http: &ureq::Agent, endpoint: &str, client: Value, body: Value) -> Option<Value> {
    let mut body = body;
    body["context"] = json!({ "client": client });
    let mut response = match http
        .post(format!("{BASE}/{endpoint}?prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .send_json(&body)
    {
        Ok(r) => r,
        Err(err) => {
            crate::log(format!("lyrics error: YouTube Music {endpoint}: {err}"));
            return None;
        }
    };
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        crate::log(format!("lyrics error: YouTube Music {endpoint}: HTTP {status}"));
        return None;
    }
    let Ok(text) = response.body_mut().with_config().limit(ANSWER_LIMIT).read_to_string() else {
        crate::log(format!("lyrics error: YouTube Music {endpoint}: answer unreadable or too big"));
        return None;
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(err) => {
            crate::log(format!("lyrics error: YouTube Music {endpoint}: not JSON ({err})"));
            None
        }
    }
}

/// Every value under `key`, anywhere in `v`, in document order.
fn find<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    out.push(child);
                }
                find(child, key, out);
            }
        }
        Value::Array(list) => list.iter().for_each(|child| find(child, key, out)),
        _ => {}
    }
}

fn all<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    find(v, key, &mut out);
    out
}

/// "3:27" or "1:02:03" → seconds.
fn clock(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(parts.iter().fold(0.0, |total, p| total * 60.0 + p.parse::<f64>().unwrap_or(0.0)))
}

/// A search hit: its video, its title and its length.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub video_id: String,
    pub title: String,
    pub duration: Option<f64>,
}

/// The song hits of a search answer, in its order.
pub fn hits(answer: &Value) -> Vec<Hit> {
    all(answer, "musicResponsiveListItemRenderer")
        .into_iter()
        .filter_map(|item| {
            let video_id = item.get("playlistItemData")?.get("videoId")?.as_str()?.to_string();
            let texts: Vec<&str> = all(item.get("flexColumns")?, "text").into_iter().filter_map(Value::as_str).collect();
            let title = texts.first()?.to_string();
            let duration = texts.iter().find_map(|t| clock(t));
            Some(Hit { video_id, title, duration })
        })
        .collect()
}

fn is_instrumental(title: &str) -> bool {
    let t = title.to_lowercase();
    ["instrumental", "off vocal", "karaoke", "inst."].iter().any(|w| t.contains(w))
}

/// The hit to take: about the track's length (any length when the player
/// doesn't say), and not an instrumental unless the track is one.
pub fn pick<'a>(hits: &'a [Hit], query: &Query) -> Option<&'a Hit> {
    let wants_instrumental = is_instrumental(&query.title);
    hits.iter().take(HITS).find(|h| {
        let near = query.duration <= 0.0 || h.duration.is_some_and(|d| (d - query.duration).abs() <= DURATION_SLACK);
        near && is_instrumental(&h.title) == wants_instrumental
    })
}

/// The timed lyrics of a lyrics page; "♪" lines become pauses.
pub fn timed_lines(page: &Value) -> Vec<Line> {
    let Some(Value::Array(cues)) = all(page, "timedLyricsData").into_iter().next() else { return Vec::new() };
    let mut lines: Vec<Line> = cues
        .iter()
        .filter_map(|cue| {
            let text = cue.get("lyricLine")?.as_str()?.trim();
            let start = cue.get("cueRange")?.get("startTimeMilliseconds")?;
            let ms = start.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| start.as_f64())?;
            let text = if text == "♪" { "" } else { text };
            Some(Line { time: ms / 1000.0, text: text.to_string() })
        })
        .collect();
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

/// The track's synced lyrics from YouTube Music: Found, None, or Error when
/// YouTube could not be asked.
pub fn fetch(http: &ureq::Agent, query: &Query) -> Lyrics {
    let title = query.title.trim();
    if title.is_empty() {
        return Lyrics::of(Status::None);
    }
    let none = Lyrics::of(Status::None);
    let search = format!("{title} {}", query.artist.trim());
    let Some(answer) = call(http, "search", web_client(), json!({ "query": search.trim(), "params": SONGS_FILTER })) else {
        return Lyrics::of(Status::Error);
    };
    let found = hits(&answer);
    let Some(hit) = pick(&found, query) else {
        let seen: Vec<String> = found
            .iter()
            .take(HITS)
            .map(|h| format!("\"{}\" {}", h.title, h.duration.map_or("?".into(), |d| format!("{d:.0} s"))))
            .collect();
        crate::log(format!("lyrics: YouTube Music has no song of about {:.0} s; saw {}", query.duration, if seen.is_empty() { "nothing".into() } else { seen.join(", ") }));
        return none;
    };
    let Some(next) = call(http, "next", android_client(), json!({ "videoId": hit.video_id })) else {
        return Lyrics::of(Status::Error);
    };
    let Some(browse_id) = all(&next, "browseId").into_iter().filter_map(Value::as_str).find(|b| b.starts_with("MPLY")) else {
        crate::log(format!("lyrics: YouTube Music \"{}\" ({}) has no lyrics tab", hit.title, hit.video_id));
        return none;
    };
    let Some(page) = call(http, "browse", android_client(), json!({ "browseId": browse_id })) else {
        return Lyrics::of(Status::Error);
    };
    let lines = timed_lines(&page);
    if lines.iter().all(|l| l.text.is_empty()) {
        crate::log(format!("lyrics: YouTube Music \"{}\" ({}) has no synced lyrics (plain or none)", hit.title, hit.video_id));
        return none;
    }
    Lyrics { status: Status::Found, lines, duration: hit.duration.unwrap_or(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(title: &str, duration: f64) -> Query {
        Query { title: title.into(), artist: "Shirakami Fubuki".into(), album: String::new(), duration }
    }

    fn item(video: &str, title: &str, length: &str) -> Value {
        json!({ "musicResponsiveListItemRenderer": {
            "playlistItemData": { "videoId": video },
            "flexColumns": [
                { "c": { "text": { "runs": [{ "text": title }] } } },
                { "c": { "text": { "runs": [{ "text": "Shirakami Fubuki" }, { "text": " • " }, { "text": length }, { "text": "492K plays" }] } } },
            ],
        } })
    }

    #[test]
    fn clock_reads_minutes_and_hours() {
        assert_eq!(clock("3:27"), Some(207.0));
        assert_eq!(clock("1:02:03"), Some(3723.0));
        assert_eq!(clock("492K plays"), None);
        assert_eq!(clock(" • "), None);
    }

    #[test]
    fn the_pick_is_the_song_of_about_the_length_not_its_instrumental() {
        let answer = json!({ "contents": [item("inst", "FOXXXROOM (Instrumental)", "3:27"), item("long", "FOXXXROOM", "4:30"), item("song", "FOXXXROOM", "3:27")] });
        let found = hits(&answer);
        assert_eq!(found[0], Hit { video_id: "inst".into(), title: "FOXXXROOM (Instrumental)".into(), duration: Some(207.0) });
        assert_eq!(pick(&found, &query("FOXXXROOM", 208.0)).unwrap().video_id, "song");
        assert_eq!(pick(&found, &query("FOXXXROOM (Instrumental)", 207.0)).unwrap().video_id, "inst");
        assert!(pick(&found, &query("FOXXXROOM", 100.0)).is_none(), "no hit of about the length");
    }

    #[test]
    fn timed_lyrics_become_lines_and_notes_become_pauses() {
        let page = json!({ "x": { "timedLyricsData": [
            { "lyricLine": "目覚めたのは", "cueRange": { "startTimeMilliseconds": "10000" } },
            { "lyricLine": "ここはきっと夢の中ね", "cueRange": { "startTimeMilliseconds": "250" } },
            { "lyricLine": "♪", "cueRange": { "startTimeMilliseconds": "4780" } },
        ] } });
        let lines = timed_lines(&page);
        assert_eq!(lines.iter().map(|l| (l.time, l.text.as_str())).collect::<Vec<_>>(), [(0.25, "ここはきっと夢の中ね"), (4.78, ""), (10.0, "目覚めたのは")]);
    }

    /// Asks YouTube Music for real: `cargo test -p fubunotch -- --ignored --nocapture live_ytm`
    #[test]
    #[ignore]
    fn live_ytm_finds_foxxxroom() {
        let http = crate::agent();
        // FUBU_YTM="title|artist|seconds" asks for another song.
        let q = match std::env::var("FUBU_YTM") {
            Ok(v) => {
                let p: Vec<&str> = v.split('|').collect();
                Query { title: p[0].into(), artist: p[1].into(), album: String::new(), duration: p[2].parse().unwrap() }
            }
            Err(_) => query("FOXXXROOM", 207.0),
        };
        let lyrics = fetch(&http, &q);
        eprintln!("{:?}: {} lines, first {:?}", lyrics.status, lyrics.lines.len(), lyrics.lines.first());
        assert_eq!(lyrics.status, Status::Found);
    }
}
